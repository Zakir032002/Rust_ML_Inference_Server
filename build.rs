

fn main()->Result<(),Box<dyn std::error::Error>>{   // here in rust all the errors implement Error trait and dyn means we dont know what type of error at compile time so we use dyn,,means dynamic error,,can be anything
         println!("BUILD SCRIPT RUNNING!");                                             //,,ok but why Box? -> we dont know the size of the error at compile time,,so we point that error by putting it into a box,,so that the key to the box is alwas the same size
       tonic_prost_build::configure()
            .build_server(true)
            .build_client(false)
            .compile_protos(&["proto/inference.proto"], &["proto"])?;

          
       
       println!("cargo:rerun-if-changed=proto/inference.proto");

       Ok(())
        
}