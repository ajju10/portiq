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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_weight_distribution() {
        let upstreams = vec![
            Upstream {
                url: "server1".to_string(),
                weight: 3,
            },
            Upstream {
                url: "server2".to_string(),
                weight: 1,
            },
        ];
        let lb = WeightedRoundRobin::new(&upstreams);

        let mut counts = HashMap::new();
        for _ in 0..1000 {
            if let Some(upstream) = lb.select() {
                *counts.entry(upstream.url.clone()).or_insert(0) += 1;
            }
        }

        // Should be around 75% server1, 25% server2
        assert!(counts["server1"] > 700 && counts["server1"] < 800);
        assert!(counts["server2"] > 200 && counts["server2"] < 300);
    }

    #[test]
    fn test_round_robin_cycle() {
        let upstreams = vec![
            Upstream {
                url: "server1".to_string(),
                weight: 1,
            },
            Upstream {
                url: "server2".to_string(),
                weight: 1,
            },
        ];
        let lb = WeightedRoundRobin::new(&upstreams);

        let server1 = lb.select().unwrap();
        let server2 = lb.select().unwrap();
        let server3 = lb.select().unwrap();

        assert_eq!(server1.url, upstreams[0].url);
        assert_eq!(server2.url, upstreams[1].url);
        assert_eq!(server3.url, upstreams[0].url);
    }

    #[test]
    fn test_no_upstream_returns_none() {
        let upstreams = vec![];
        let lb = WeightedRoundRobin::new(&upstreams);
        assert!(lb.select().is_none())
    }

    #[test]
    fn test_zero_weight_returns_none() {
        let upstreams = vec![
            Upstream {
                url: "server1".to_string(),
                weight: 0,
            },
            Upstream {
                url: "server2".to_string(),
                weight: 0,
            },
        ];
        let lb = WeightedRoundRobin::new(&upstreams);
        assert!(lb.select().is_none())
    }
}
