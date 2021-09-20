#[cfg(all(feature = "tokio", feature = "async-std"))]
compile_error!("`tokio` and `async-std` features must not be enabled together");

use future::Retry;
use std::future::Future;

pub mod backoff;
pub mod future;
pub mod policies;
#[cfg(any(feature = "tokio", feature = "async-std"))]
pub mod sleep;

pub fn retry<P, T, E, F, CF>(create_f: CF, policy: P) -> Retry<P, T, E, F, CF>
where
    P: Policy<T, E>,
    F: Future<Output = Result<T, E>>,
    CF: CreateFuture<F>,
{
    Retry::new(policy, create_f)
}

pub trait CreateFuture<F> {
    fn create(&mut self) -> F;
}

impl<F, FN> CreateFuture<F> for FN
where
    FN: FnMut() -> F,
{
    fn create(&mut self) -> F {
        self()
    }
}

pub trait Policy<T, E>: Sized {
    type ForceRetryFuture: Future;
    type RetryFuture: Future<Output = Self>;

    fn force_retry_after(&self) -> Self::ForceRetryFuture;

    fn retry(self, result: Option<Result<&T, &E>>) -> Option<Self::RetryFuture>;
}

#[cfg(test)]
mod tests {
    use crate::{retry, Policy};
    use std::future::pending;
    use std::{
        future::{ready, Future, Ready},
        pin::Pin,
        sync::{Arc, Mutex},
    };

    #[cfg(all(feature = "tokio", test))]
    pub fn run_test(f: impl std::future::Future + Send + 'static) {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(f);
    }

    #[cfg(all(feature = "async-std", test))]
    pub fn run_test(f: impl std::future::Future + Send + 'static) {
        async_std::task::block_on(f);
    }

    #[cfg(all(feature = "tokio", test))]
    pub use tokio::task::spawn;

    #[cfg(all(feature = "async-std", test))]
    pub use async_std::task::spawn;

    #[cfg(all(feature = "tokio", test))]
    pub use tokio::task::yield_now;

    #[cfg(all(feature = "async-std", test))]
    pub use async_std::task::yield_now;

    #[test]
    fn should_drop_previous_delay_after_retry() {
        struct RetryTest {
            retry: usize,
        }

        impl<T, E> Policy<T, E> for RetryTest {
            type ForceRetryFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
            type RetryFuture = Ready<Self>;

            fn force_retry_after(&self) -> Self::ForceRetryFuture {
                if self.retry == 1 {
                    Box::pin(pending())
                } else {
                    Box::pin(ready(()))
                }
            }

            fn retry(self, _result: Option<Result<&T, &E>>) -> Option<Self::RetryFuture> {
                if self.retry == 5 {
                    return None;
                } else {
                    Some(ready(Self {
                        retry: self.retry + 1,
                    }))
                }
            }
        }
        run_test(async {
            let call_count = Arc::new(Mutex::new(0));
            let create_fut = || {
                let call_count = call_count.clone();
                async move {
                    let call = {
                        let mut mutex_guard = call_count.lock().unwrap();
                        *mutex_guard += 1;
                        *mutex_guard
                    };
                    if call == 2 {
                        crate::tests::yield_now().await;
                        Err(())
                    } else if call == 3 {
                        pending().await
                    } else {
                        Err::<(), ()>(())
                    }
                }
            };

            let _ = retry(create_fut, RetryTest { retry: 0 }).await;

            let guard = call_count.lock().unwrap();
            assert_eq!(*guard, 6);
        })
    }
}
