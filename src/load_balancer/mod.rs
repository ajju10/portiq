use crate::config::Upstream;
use std::sync::atomic::{AtomicUsize, Ordering};

pub trait LoadBalancerStrategy: Send + Sync {
    fn select(&self) -> Option<&Upstream>;
}

pub struct WeightedRoundRobin {
    upstreams: Box<[Upstream]>,
    weighted: Box<[u16]>,
    next_index: AtomicUsize,
}

impl WeightedRoundRobin {
    pub fn new(upstreams: &[Upstream]) -> Self {
        assert!(
            upstreams.len() <= u16::MAX as usize,
            "support up to 2^16 upstreams"
        );

        let servers = upstreams.to_owned().into_boxed_slice();
        let mut weighted = Vec::with_capacity(servers.len());
        for (index, server) in servers.iter().enumerate() {
            for _ in 0..server.weight {
                weighted.push(index as u16);
            }
        }

        WeightedRoundRobin {
            upstreams: servers,
            weighted: weighted.into_boxed_slice(),
            next_index: AtomicUsize::new(0),
        }
    }
}

impl LoadBalancerStrategy for WeightedRoundRobin {
    fn select(&self) -> Option<&Upstream> {
        if self.weighted.is_empty() {
            return None;
        }

        let next_index = self.next_index.load(Ordering::Relaxed);
        let upstream_index = self.weighted[next_index] as usize;
        self.next_index
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |x| {
                Some((x + 1) % self.weighted.len())
            })
            .unwrap_or(0);
        Some(&self.upstreams[upstream_index])
    }
}

pub struct LoadBalancer {
    strategy: Box<dyn LoadBalancerStrategy>,
}

impl LoadBalancer {
    pub fn new(strategy: Box<dyn LoadBalancerStrategy>) -> Self {
        LoadBalancer { strategy }
    }

    pub fn get_next(&self) -> Option<&Upstream> {
        self.strategy.select()
    }
}
