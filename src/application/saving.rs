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
    pub revision: u64,
    pub destination: SaveDestination,
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
        revision: u64,
        destination: SaveDestination,
    ) -> SaveDecision {
        if let Some(ticket) = &mut self.in_flight {
            ticket.destination = ticket.destination.max(destination);
            return SaveDecision::Idle;
        }
        let ticket = SaveTicket {
            id,
            revision,
            destination,
        };
        self.in_flight = Some(ticket.clone());
        SaveDecision::Start(ticket)
    }

    pub fn ticket(&self) -> Option<&SaveTicket> {
        self.in_flight.as_ref()
    }

    pub fn complete(&mut self, completion: SaveCompletion, current_revision: u64) -> SaveDecision {
        let Some(ticket) = self.in_flight.take() else {
            return SaveDecision::Idle;
        };
        if completion == SaveCompletion::Failed {
            return SaveDecision::Failed(ticket.destination);
        }
        if ticket.destination != SaveDestination::Background && current_revision != ticket.revision
        {
            return self.request(ticket.id, current_revision, ticket.destination);
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
        assert!(matches!(
            coordinator.request(id(), 2, SaveDestination::Background),
            SaveDecision::Start(_)
        ));
        coordinator.request(id(), 2, SaveDestination::Exit);
        assert_eq!(
            coordinator.ticket().unwrap().destination,
            SaveDestination::Exit
        );
        assert!(matches!(
            coordinator.complete(SaveCompletion::Succeeded, 3),
            SaveDecision::Start(SaveTicket {
                revision: 3,
                destination: SaveDestination::Exit,
                ..
            })
        ));
    }
}
