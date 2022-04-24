use std::thread;
use crossbeam::utils::Backoff;
use spin::RwLock;

pub fn guard<T,F,R>(spin_lock:&RwLock<T>,f:F)->R
    where  F:Fn(&RwLock<T>)-> Option<R>
{
    let backoff = Backoff::new();
    loop{

            if let Some(guard) = f(spin_lock){
                return guard;
            }

        backoff.spin();
    }
}