use crate::application::WorldId;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum SaveDestination {
    #[default]
    Background,
    MainMenu,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveTicket {
    pub id: WorldId,
    pub version: SaveVersion,
    pub destination: SaveDestination,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SaveVersion {
    pub world_revision: u64,
    pub day_time_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveCompletion {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveDecision {
    Idle,
    Start(SaveTicket),
    Finish(SaveDestination),
    Failed(SaveDestination),
}

#[derive(Debug, Default)]
pub struct SaveCoordinator {
    in_flight: Option<SaveTicket>,
}

impl SaveCoordinator {
    pub fn request(
        &mut self,
        id: WorldId,
        version: SaveVersion,
        destination: SaveDestination,
    ) -> SaveDecision {
        if let Some(ticket) = &mut self.in_flight {
            ticket.destination = ticket.destination.max(destination);
            return SaveDecision::Idle;
        }
        let ticket = SaveTicket {
            id,
            version,
            destination,
        };
        self.in_flight = Some(ticket.clone());
        SaveDecision::Start(ticket)
    }

    pub fn ticket(&self) -> Option<&SaveTicket> {
        self.in_flight.as_ref()
    }

    pub fn complete(
        &mut self,
        completion: SaveCompletion,
        current_version: SaveVersion,
    ) -> SaveDecision {
        let Some(ticket) = self.in_flight.take() else {
            return SaveDecision::Idle;
        };
        if completion == SaveCompletion::Failed {
            return SaveDecision::Failed(ticket.destination);
        }
        if ticket.destination != SaveDestination::Background && current_version != ticket.version {
            return self.request(ticket.id, current_version, ticket.destination);
        }
        SaveDecision::Finish(ticket.destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> WorldId {
        WorldId::new("world-1").unwrap()
    }

    #[test]
    fn destinations_escalate_and_latest_revision_is_saved_before_exit() {
        let mut coordinator = SaveCoordinator::default();
        let first = SaveVersion {
            world_revision: 2,
            day_time_ticks: 100,
        };
        assert!(matches!(
            coordinator.request(id(), first, SaveDestination::Background),
            SaveDecision::Start(_)
        ));
        coordinator.request(id(), first, SaveDestination::Exit);
        assert_eq!(
            coordinator.ticket().unwrap().destination,
            SaveDestination::Exit
        );
        assert!(matches!(
            coordinator.complete(
                SaveCompletion::Succeeded,
                SaveVersion {
                    world_revision: 3,
                    day_time_ticks: 120,
                }
            ),
            SaveDecision::Start(SaveTicket {
                version: SaveVersion {
                    world_revision: 3,
                    day_time_ticks: 120,
                },
                destination: SaveDestination::Exit,
                ..
            })
        ));
    }

    #[test]
    fn clock_change_alone_restarts_an_escalated_save() {
        let mut coordinator = SaveCoordinator::default();
        let captured = SaveVersion {
            world_revision: 4,
            day_time_ticks: 1_000,
        };
        coordinator.request(id(), captured, SaveDestination::Background);
        coordinator.request(id(), captured, SaveDestination::MainMenu);
        assert!(matches!(
            coordinator.complete(
                SaveCompletion::Succeeded,
                SaveVersion {
                    day_time_ticks: 1_001,
                    ..captured
                }
            ),
            SaveDecision::Start(SaveTicket {
                version: SaveVersion {
                    day_time_ticks: 1_001,
                    ..
                },
                destination: SaveDestination::MainMenu,
                ..
            })
        ));
    }
}
