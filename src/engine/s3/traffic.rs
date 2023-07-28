use crate::cli::TrafficPattern;
use crate::engine::s3::uri::UriProvider;
use hyper::Uri;
use std::mem;

#[derive(Debug, Clone)]
pub enum TrafficState {
    Put { uri: Uri },
    Get { uri: Uri },
}

pub struct TrafficStateMachine {
    pattern: TrafficPattern,
    uri_supplier: UriProvider,
    state: TrafficState,
}

impl TrafficStateMachine {
    pub fn new(pattern: TrafficPattern, mut uri_supplier: UriProvider) -> Self {
        let state = match pattern {
            TrafficPattern::Both | TrafficPattern::Put => TrafficState::Put {
                uri: uri_supplier.next(),
            },
            TrafficPattern::Get => TrafficState::Get {
                uri: uri_supplier.next(),
            },
        };
        TrafficStateMachine {
            pattern,
            uri_supplier,
            state,
        }
    }

    pub fn next(&mut self) -> TrafficState {
        let new_state = match &self.pattern {
            // If we're in a PUT traffic pattern, keep issuing PUTs
            TrafficPattern::Put => TrafficState::Put {
                uri: self.uri_supplier.next(),
            },
            // If we're in a GET traffic pattern, keep issuing GETs
            TrafficPattern::Get => TrafficState::Get {
                uri: self.uri_supplier.next(),
            },
            // If we're in a BOTH traffic pattern, switch between PUTs and GETs, starting
            // with PUTs to ensure the object exists
            TrafficPattern::Both => match &self.state {
                // Take the URI from the PUT we just issued and use it for our next GET request
                TrafficState::Put { uri } => TrafficState::Get { uri: uri.clone() },
                TrafficState::Get { .. } => TrafficState::Put {
                    uri: self.uri_supplier.next(),
                },
            },
        };
        mem::replace(&mut self.state, new_state)
    }
}

#[cfg(test)]
mod tests {
    use crate::cli::TrafficPattern;
    use crate::engine::s3::traffic::TrafficState;
    use crate::engine::s3::traffic::TrafficStateMachine;
    use crate::engine::s3::uri::UriProvider;

    #[test]
    fn put_traffic_pattern() {
        let mut expected_uri_provider =
            UriProvider::new(String::new(), String::new(), String::new(), 0, 1, 1);
        let mut machine =
            TrafficStateMachine::new(TrafficPattern::Put, expected_uri_provider.clone());

        for _ in 0..1000 {
            let next_uri = expected_uri_provider.next();
            assert!(matches!(machine.next(), TrafficState::Put { uri } if uri == next_uri));
        }
    }

    #[test]
    fn get_traffic_pattern() {
        let mut expected_uri_provider =
            UriProvider::new(String::new(), String::new(), String::new(), 0, 1, 1);
        let mut machine =
            TrafficStateMachine::new(TrafficPattern::Get, expected_uri_provider.clone());

        for _ in 0..1000 {
            let next_uri = expected_uri_provider.next();
            assert!(matches!(machine.next(), TrafficState::Get { uri } if uri == next_uri));
        }
    }

    #[test]
    fn both_traffic_pattern() {
        let mut expected_uri_provider =
            UriProvider::new(String::new(), String::new(), String::new(), 0, 1, 1);
        let mut machine =
            TrafficStateMachine::new(TrafficPattern::Both, expected_uri_provider.clone());

        let mut last_state = None;
        for _ in 0..1000 {
            let next_state = machine.next();
            match last_state {
                None => {
                    let next_uri = expected_uri_provider.next();
                    assert!(
                        matches!(next_state.clone(), TrafficState::Put { uri }  if uri == next_uri)
                    );
                }
                Some(s) => match s {
                    TrafficState::Put { uri: last_uri } => {
                        assert!(
                            matches!(next_state.clone(), TrafficState::Get { uri } if uri == last_uri)
                        );
                    }
                    TrafficState::Get { .. } => {
                        let next_uri = expected_uri_provider.next();
                        assert!(
                            matches!(next_state.clone(), TrafficState::Put { uri } if uri == next_uri)
                        );
                    }
                },
            }

            last_state = Some(next_state);
        }
    }
}
