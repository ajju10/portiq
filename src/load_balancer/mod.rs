use crate::config::Upstream;

pub trait LoadBalancerStrategy: Send {
    fn select(&mut self) -> Option<&Upstream>;
}

pub struct WeightedRoundRobin {
    upstreams: Box<[Upstream]>,
    weighted: Box<[u16]>,
    next_index: usize,
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
            next_index: 0,
        }
    }
}

impl LoadBalancerStrategy for WeightedRoundRobin {
    fn select(&mut self) -> Option<&Upstream> {
        if self.weighted.is_empty() {
            return None;
        }

        let index = self.weighted[self.next_index] as usize;
        self.next_index = (self.next_index + 1) % self.weighted.len();
        Some(&self.upstreams[index])
    }
}

pub struct LoadBalancer {
    strategy: Box<dyn LoadBalancerStrategy + Send>,
}

impl LoadBalancer {
    pub fn new(strategy: Box<dyn LoadBalancerStrategy>) -> Self {
        LoadBalancer { strategy }
    }

    pub fn get_next(&mut self) -> Option<&Upstream> {
        self.strategy.select()
    }
}
