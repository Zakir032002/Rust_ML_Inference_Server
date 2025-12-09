use crate::inference::InferenceEngine;
use crate::batching::DynamicBatcher;

use std::sync::{Arc};

use tonic::{Request,Response,Status};
use ndarray::{Array,IxDyn};

use futures::future::join_all;
use std::time::Instant;

pub mod proto{
    tonic::include_proto!("inference");
}

pub use proto::{
    BatchRequest,BatchResponse,PredictRequest,PredictResponse,
    inference_service_server::InferenceService
};

#[derive(Clone)]
pub struct InferenceServer{
    engine: Arc<InferenceEngine>,
    batcher: Arc<DynamicBatcher>
}

impl InferenceServer{
    pub fn new(engine:Arc<InferenceEngine>,batcher:Arc<DynamicBatcher>)->Self{
        Self { engine , batcher }
    }
}

#[tonic::async_trait]
impl InferenceService for InferenceServer{
    async fn predict(&self,request:Request<PredictRequest>)->Result<Response<PredictResponse>,Status>{
        let start = Instant::now();
        let payload = request.into_inner();

        let shape:Vec<usize> = payload.shape.iter().map(|&x| x as usize).collect();

        let input = Array::from_shape_vec(IxDyn(&shape), payload.data).map_err(|e|Status::invalid_argument(format!("error while converting vec t0 ndarray")))?;

        let (output,output_shape) = self.batcher.submit(input).await.map_err(|e|Status::internal(format!("batcher_error {e}")))?;

        let latency = start.elapsed().as_secs_f32() * 1000.0;

        Ok(Response::new(
            PredictResponse { predictions: output, shape: output_shape, latency_ms: latency }
        ))

    }

    async fn batch_predict(&self,batch_request:Request<BatchRequest>)->Result<Response<BatchResponse>,Status>{
        let batch = batch_request.into_inner();
        let mut tasks = Vec::new();

        for request in batch.requests{
            let srv = self.clone();

            tasks.push(tokio::spawn(async move {
                srv.predict(Request::new(request)).await
            }));

        }
        let results = join_all(tasks).await;
        let mut responses = Vec::new();

        for res in results{
            let result = res.map_err(|e| Status::internal(format!("{}", e)))?;

            match result {
                Ok(resp) => responses.push(resp.into_inner()),
                Err(status) => return Err(status),
            }
        }

         Ok(Response::new(BatchResponse {
            response: responses,
        }))

    }
}
