use anyhow::{Context, Result};
use ndarray::{Array, IxDyn};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::TensorRef;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::time::Instant;
use num_cpus; 

//the point here is to use the cpu to the fullest and hence every system has unique spu cores thus it iwll automatically calculates the cores and assign sessions

//  Atomic operations (lock-free concurrency)

//  Thread-safe pooling (Mutex-based)

//  Load balancing (Round-Robin algorithm)

//  Backpressure (max concurrency limit)

//  Auto-configuration (CPU-aware tuning)

fn calculate_pool_size() -> usize { //this calculates 
    let cores = num_cpus::get();
    (cores / 2).max(2).min(8)
}

fn calculate_intra_threads() -> i32 {
    let cores = num_cpus::get();
    let auto = (cores / 4).max(1).min(4) as i32;
    auto
}

fn calculate_max_concurrency(pool: usize) -> usize {
    std::env::var("MAX_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(pool * 3)
}

pub struct InferenceEngine {
    sessions: Vec<Mutex<Session>>,
    input_shape: Vec<i64>,
    pub index : AtomicUsize, //round robin counter
    pub active_requests : AtomicUsize, //concurrency tracker
    pub max_concurrency : usize //backpressure
}

impl InferenceEngine {
    pub fn new(model_filepath: &Path) -> Result<Self> {

        let cores = num_cpus::get();
        let pool_size = calculate_pool_size();
        let intra_threads = calculate_intra_threads();
        let max_concurrency = calculate_max_concurrency(pool_size);

        println!("Metrics : ");
        println!("  CPU cores       : {}", cores);
        println!("  Session pool    : {}", pool_size);
        println!("  Threads/session : {}", intra_threads);
        println!("  Max concurrency : {}", max_concurrency);

        let mut sessions = Vec::with_capacity(pool_size);

        for i in 0..pool_size{
            let session = Session::builder()?
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .with_intra_threads(intra_threads as usize)?
                .commit_from_file(model_filepath)
                .context("Failed to load this session")?;
            println!("Session {i} loaded successfully");
            sessions.push(Mutex::new(session));
        }

        // Extract input shape from model metadata
        let input_shape = sessions[0].lock().unwrap().inputs[0]
            .input_type
            .tensor_shape()
            .context("No tensorshape available")?
            .to_vec();

        

        Ok(Self {
            sessions,
            input_shape,
            index : AtomicUsize::new(0),
            active_requests : AtomicUsize::new(0),
            max_concurrency
        })
     }

    pub fn infer(&self, input: Array<f32, IxDyn>) -> Result<(Vec<f32>,Vec<i64>)> {
        // Validate input shape: should be 4D and match model static dims
        let input_dims = input.shape();
        anyhow::ensure!(
            input_dims.len() == self.input_shape.len(),
            "Input rank mismatch: expected {} dimensions, got {}",
            self.input_shape.len(),
            input_dims.len()
        );
        
        let current = self.active_requests.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        //implementing backpressure to avoid infinate queue or else you'll be cooked fr 😎
        if current >= self.max_concurrency{
            self.active_requests.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            anyhow::bail!("Too many concurrent requests");
        }

        let start_time = Instant::now();

        

            // Convert IxDyn to Ix4; this is required by the ort API for TensorRef
            let input_fixed = input
                  .into_dimensionality::<ndarray::Ix4>()
                  .context("Input must be 4-dimensional for ResNet-50")?;

            // Run inference
            let shape: Vec<i64> = input_fixed.shape().iter().map(|d| *d as i64).collect();
            let data: &[f32] = input_fixed.as_slice()
                  .expect("Input tensor must be contiguous");

            
            let session_mutex = self.select_session();
            let mut session = session_mutex.lock().unwrap(); //locking when the session started and it rel;eases the lock automatically

            let outputs = session.run(
                            ort::inputs![TensorRef::from_array_view((shape, data))?]
                        )?;

        
            


            let (out_shape, out_data) = outputs["resnetv24_dense0_fwd"]
                        .try_extract_tensor::<f32>()?;
                        
            
            let output = out_data.to_vec();

            let output_shape = out_shape.to_vec();
            let duration = start_time.elapsed();
            println!(" Inference finished in {:?}", duration);
            
            self.active_requests.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);




        Ok((output,output_shape))
    }

    //round robin to schedule the requests
    pub fn select_session(&self)->&Mutex<Session>{
        let i = self.index.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % self.sessions.len();
        &self.sessions[i]
    }

    


    
}




#[cfg(test)]
mod tests{
    use ndarray::Array;

    

    use super::*;
    
    #[test]
    fn engine_activation_check(){
        let k = match InferenceEngine::new(Path::new("models/resnet50.onnx")){
            Ok(result) => format!("the inference loaded successfully and the input shape is {:?} ",result.input_shape),
            Err(e)=> format!("there is an error {}",e)
        };
        print!("{}",k);
    }

    #[test]
    fn inference_check(){
        let mut engine = InferenceEngine::new(Path::new("models/resnet50.onnx")).unwrap();


        let dummy_input:Array<f32,IxDyn> = ndarray::Array::zeros(IxDyn(&[5,3,224,224]));

        let (output,output_shape) = engine.infer(dummy_input).unwrap();

        println!("the output is like {:?}, and the output shape is {:?}",&output[1..10],output_shape);

    }

   
}