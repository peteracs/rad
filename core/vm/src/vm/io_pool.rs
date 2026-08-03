use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

enum Message {
    Run(Job),
    Shutdown,
}

pub struct IoPool {
    tx: Sender<Message>,
    workers: Vec<thread::JoinHandle<()>>,
}

impl IoPool {
    pub fn new(worker_count: usize) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            let (tx, _) = mpsc::channel::<Message>();
            Self {
                tx,
                workers: Vec::new(),
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = mpsc::channel::<Message>();
            let shared_rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
            let mut workers = Vec::with_capacity(worker_count);
            for _ in 0..worker_count {
                let worker_rx = shared_rx.clone();
                workers.push(thread::spawn(move || loop {
                    let msg = {
                        let lock = worker_rx.lock();
                        if let Ok(guard) = lock {
                            guard.recv()
                        } else {
                            return;
                        }
                    };
                    match msg {
                        Ok(Message::Run(job)) => job(),
                        Ok(Message::Shutdown) | Err(_) => return,
                    }
                }));
            }
            Self { tx, workers }
        }
    }

    pub fn submit<F, T>(&self, work: F) -> Receiver<Result<T, String>>
    where
        F: FnOnce() -> Result<T, String> + Send + 'static,
        T: Send + 'static,
    {
        let (result_tx, result_rx) = mpsc::channel::<Result<T, String>>();
        let job = Box::new(move || {
            let _ = result_tx.send(work());
        });
        let _ = self.tx.send(Message::Run(job));
        result_rx
    }

    /// Construct a pool that owns no worker threads.
    ///
    /// Parallel/simulation worker VMs are not allowed to perform I/O, so
    /// giving every Rayon worker a nested OS thread was both wasteful and, on
    /// Windows process teardown, unsound operationally: a thread-local worker
    /// VM could try to join its nested I/O thread while the runtime was
    /// already destroying process threads. The standard library then aborts
    /// with `threads should not terminate unexpectedly`.
    pub fn disabled() -> Self {
        Self::new(0)
    }

    #[cfg(test)]
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }
}

impl Drop for IoPool {
    fn drop(&mut self) {
        for _ in 0..self.workers.len() {
            let _ = self.tx.send(Message::Shutdown);
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IoPool;

    #[test]
    fn disabled_pool_owns_no_threads() {
        let pool = IoPool::disabled();
        assert_eq!(pool.worker_count(), 0);
    }
}
