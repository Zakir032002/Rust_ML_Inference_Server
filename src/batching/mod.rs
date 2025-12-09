use std::sync::{
      Arc,atomic::{AtomicUsize,Ordering}
};
use ndarray::{IxDyn,Array,Ix4,Axis,stack};
use anyhow::Result;
use tokio::time::{sleep,Duration,Instant};
use tokio::sync::{oneshot,mpsc};
use metrics::{counter, gauge, histogram};
 // if not already present

use crate::inference::InferenceEngine;

//its like a ticket ,,when we get a request from user we store create a BatchItem with the input we recieved and where to send back the result
pub struct BatchItem{
      input : Array<f32,IxDyn>,
      tx : oneshot::Sender<Result<(Vec<f32>,Vec<i64>)>>,
      queued_at: Instant,  
}

pub struct BatchMetrics {
      pub queue_depth: AtomicUsize,
      pub total_batches: AtomicUsize,
      pub total_items: AtomicUsize,
      pub queue_wait_ns: AtomicUsize,
      pub batch_latency_ns: AtomicUsize,
      pub infer_latency_ns: AtomicUsize,
}

impl BatchMetrics {
    pub fn new() -> Self {
        Self {
            queue_depth: AtomicUsize::new(0), //queue depth in mspc channel
            total_batches: AtomicUsize::new(0),
            total_items: AtomicUsize::new(0),
            queue_wait_ns: AtomicUsize::new(0),
            batch_latency_ns: AtomicUsize::new(0),
            infer_latency_ns: AtomicUsize::new(0),
            
        }
    }

    pub fn avg_batch_size(&self) -> f64 {
        let batches = self.total_batches.load(Ordering::Relaxed);
        if batches == 0 {
            return 0.0;
        }
        self.total_items.load(Ordering::Relaxed) as f64 / batches as f64
    }
    
}

pub struct DynamicBatcher {
    engine: Arc<InferenceEngine>,
    tx: mpsc::Sender<BatchItem>,
    metrics: Arc<BatchMetrics>,
    max_batch: usize,
    wait_ms: u64,
}

impl DynamicBatcher {
      pub fn new(engine: Arc<InferenceEngine>,max_batch_size: usize,max_wait_ms: u64) -> Arc<Self> {

            let (tx, rx) = mpsc::channel::<BatchItem>(max_batch_size * 4);

            let this = Arc::new(Self {
                  engine,
                  tx,
                  metrics: Arc::new(BatchMetrics::new()),
                  max_batch: max_batch_size,
                  wait_ms: max_wait_ms,
            });

            let worker = this.clone();
            tokio::spawn(async move {
                  worker.run(rx).await; //move rx into run function because worker cant take ownership and mutable access to the channel
            });

            this
      }

      pub async fn submit(&self, input: Array<f32, IxDyn>) -> Result<(Vec<f32>, Vec<i64>)> {
            let (tx, rx) = oneshot::channel();

            // queue depth ++
            self.metrics.queue_depth.fetch_add(1, Ordering::Relaxed);

            self.tx.send(BatchItem { input, tx,queued_at: Instant::now() }).await
                  .map_err(|_| anyhow::anyhow!("Batch queue is full"))?;

            rx.await?
      }

      async fn run(self: Arc<Self>,mut rx:mpsc::Receiver<BatchItem>) {
            loop {
                  let mut batch_items = Vec::with_capacity(self.max_batch);

                  match rx.recv().await {
                  Some(item) => {
                        batch_items.push(item);
                        self.metrics.queue_depth.fetch_sub(1, Ordering::Relaxed);
                  }
                  None => return,

                  }
                  //setting the timer
                  let deadline = sleep(Duration::from_millis(self.wait_ms));
                  tokio::pin!(deadline); //pinning the deadline for the tokio::select

                  while batch_items.len() < self.max_batch {
                  tokio::select! {                                //this means ,,which ever happens first take that result..here its either reciving the res or expiration of the deadline
                        recv = rx.recv() => match recv {
                              Some(item) => {
                              batch_items.push(item);
                              self.metrics.queue_depth.fetch_sub(1, Ordering::Relaxed);
                              }
                              None => break,
                        },
                        _ = &mut deadline => break,
                  }
                  }
                  


                  // update metrics
                  self.metrics.total_batches.fetch_add(1, Ordering::Relaxed); //batches++ because we just created a batch of items above
                  self.metrics.total_items.fetch_add(batch_items.len(), Ordering::Relaxed);
                  // queue wait latency = time since first request entered queue
                  let wait_ns = batch_items[0].queued_at.elapsed().as_nanos() as usize;
                  self.metrics.queue_wait_ns.store(wait_ns, Ordering::Relaxed);

                  let queue_wait = batch_items[0].queued_at.elapsed().as_nanos() as usize;



                  if batch_items.len() == 1 {
                  let single = batch_items.pop().unwrap();
                  let engine = self.engine.clone(); //we should clone because tokio spawn task takes the ownership

                  tokio::task::spawn_blocking(move || {                    // not async because we only have one request and cpu intense work
                        let res = engine.infer(single.input);
                        let _ = single.tx.send(res);
                  });

                  continue;
                  }

                  // Build batched tensor-> copy 1 -> batch_items -> sample_views ex: [1,3,244,244] -> [3,244,244] for batching
                  let mut sample_views = Vec::new();
                  for it in &batch_items {
                        if let Ok(arr4) = it.input.clone().into_dimensionality::<Ix4>() {
                              if arr4.shape()[0] == 1 {
                                    sample_views.push(arr4.index_axis_move(Axis(0), 0));
                              }
                        }
                  }
                  // fallback if shapes are different ,,usually dosnt happen, handlesd in client side
                  if sample_views.len() != batch_items.len() {
                        //process each indivially
                        let engine = self.engine.clone();
                        for it in batch_items {
                              let eng = engine.clone();
                              tokio::task::spawn_blocking(move || {
                                    let res = eng.infer(it.input);
                                    let _ = it.tx.send(res);
                              });
                        }
                        continue;
                  }

                  //stack into batch tensors
                  //no copy just view is a pointer to sample_views;
                  let views: Vec<_> = sample_views.iter().map(|v| v.view()).collect();

                  let batched = match stack(Axis(0), &views) { //stacking the images [[3,244,244],...] -> [n,3,244,244]
                  Ok(b) => b.into_dyn(), // changing the shape from Array<f32,Arr4> -> Array<f32,IxDyn>
                  Err(_) => {   
                        // fallback -> if the above operation is failed
                        let engine = self.engine.clone();
                        for it in batch_items {
                              let eng = engine.clone();
                              tokio::task::spawn_blocking(move || {
                              let res = eng.infer(it.input);
                              let _ = it.tx.send(res);
                              });
                        }
                        continue;
                  }
                  };

                  let engine = self.engine.clone();
                  let metrics = self.metrics.clone(); // it is Arc::clone -> just a pointer
                  tokio::task::spawn_blocking(move || {
                        let batch_start = Instant::now();
                        let start = Instant::now();
                        let res = engine.infer(batched);
                  //inside engine :
                  // 1. Backpressure check

                  // 2. Select session (round-robin)
                  // Suppose index was 0
                  // 0 → 1, returns 0
                  // i = 0 % 4 = 0
                  // Select sessions[0]

                  // 3. Lock session

                  // 4. Convert to Ix4

                  // 5. Prepare ONNX input -> shape of the input and the data as array slice

                  //6. Run ONNX

                  // ONNX Runtime internally:
                  //   - Uses intra_threads = 2 (spawns 2 OS threads)
                  //   - Performs matrix operations in parallel
                  //   - Processes all 3 images in one forward pass
                  //   - Takes ~20-50ms

                  // 7. Extract output

                  // 8. Release session lock
                  // (session variable drops here)
                  // 🔓 Mutex unlocked!

                  // 9. Decrement backpressure
                        // record inference latency
                        let infer_ns = start.elapsed().as_nanos() as usize;
                        metrics.infer_latency_ns.store(infer_ns, Ordering::Relaxed);

                        // record total batch latency
                        let batch_ns = batch_start.elapsed().as_nanos() as usize;
                        metrics.batch_latency_ns.store(batch_ns, Ordering::Relaxed);

                        //quick question,,if we batch all the inputs then how do we know where to send each result?
                  match res {
                        Ok((flat, shape)) => {
                              let b = shape[0] as usize; //batch size
                              let per = flat.len() / b; // items per image

                              for (i, it) in batch_items.into_iter().enumerate() { //iterate and take ownership if the batchitem and add index to it (0,item),(1,item)..etc
                              let offset = i * per;
                              let slice = flat[offset..offset + per].to_vec();
                              let _ = it.tx.send(Ok((slice, vec![per as i64])));
                              }

                              println!("Batch {} done in {:?}",b,start.elapsed());
                        }
                        Err(e) => {
                              let shared_error = Arc::new(e);
                              for it in batch_items {
                                    let err = Arc::clone(&shared_error);
                                    let _ = it.tx.send(Err(anyhow::anyhow!("{}",err)));
                              }
                        }
                  }
                  });
            }
    }

    pub fn metrics(&self) -> Arc<BatchMetrics> {
        self.metrics.clone()
    }

    

}