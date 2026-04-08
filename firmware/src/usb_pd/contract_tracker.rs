#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractTracker<C>
where
    C: Copy + PartialEq + Eq,
{
    active_contract: Option<C>,
    pending_contract: Option<C>,
    waiting_for_accept: bool,
    waiting_for_ps_rdy: bool,
}

impl<C> Default for ContractTracker<C>
where
    C: Copy + PartialEq + Eq,
{
    fn default() -> Self {
        Self {
            active_contract: None,
            pending_contract: None,
            waiting_for_accept: false,
            waiting_for_ps_rdy: false,
        }
    }
}

impl<C> ContractTracker<C>
where
    C: Copy + PartialEq + Eq,
{
    pub const fn active_contract(&self) -> Option<C> {
        self.active_contract
    }

    pub const fn pending_contract(&self) -> Option<C> {
        self.pending_contract
    }

    pub const fn waiting_for_accept(&self) -> bool {
        self.waiting_for_accept
    }

    pub const fn waiting_for_ps_rdy(&self) -> bool {
        self.waiting_for_ps_rdy
    }

    pub const fn request_in_flight(&self) -> bool {
        self.waiting_for_accept || self.waiting_for_ps_rdy
    }

    pub fn refresh_source_capabilities(&mut self, preserve_pending_contract: bool) {
        if !preserve_pending_contract {
            self.cancel_pending_request();
        }
    }

    pub fn begin_request(&mut self, contract: C) {
        self.pending_contract = Some(contract);
        self.waiting_for_accept = true;
        self.waiting_for_ps_rdy = false;
    }

    pub fn mark_accept_received(&mut self) -> bool {
        if !self.waiting_for_accept {
            return false;
        }
        self.waiting_for_accept = false;
        self.waiting_for_ps_rdy = true;
        true
    }

    pub fn commit_pending_contract(&mut self) -> Option<C> {
        if !self.waiting_for_ps_rdy {
            return None;
        }
        self.waiting_for_ps_rdy = false;
        let next = self.pending_contract.take();
        self.active_contract = next;
        next
    }

    pub fn cancel_pending_request(&mut self) {
        self.pending_contract = None;
        self.waiting_for_accept = false;
        self.waiting_for_ps_rdy = false;
    }

    pub fn clear_all(&mut self) {
        self.active_contract = None;
        self.cancel_pending_request();
    }
}

#[cfg(test)]
mod tests {
    use super::ContractTracker;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TestContract {
        voltage_mv: u16,
    }

    #[test]
    fn source_caps_refresh_preserves_in_flight_contract_when_still_supported() {
        let old_contract = TestContract { voltage_mv: 9_000 };
        let new_contract = TestContract { voltage_mv: 15_000 };

        let mut tracker = ContractTracker::default();
        tracker.begin_request(old_contract);
        assert_eq!(tracker.pending_contract(), Some(old_contract));
        assert!(tracker.mark_accept_received());
        assert_eq!(tracker.commit_pending_contract(), Some(old_contract));
        assert_eq!(tracker.active_contract(), Some(old_contract));

        tracker.begin_request(new_contract);
        assert_eq!(tracker.pending_contract(), Some(new_contract));
        assert!(tracker.waiting_for_accept());
        tracker.refresh_source_capabilities(true);

        assert_eq!(tracker.active_contract(), Some(old_contract));
        assert_eq!(tracker.pending_contract(), Some(new_contract));
        assert!(tracker.request_in_flight());
    }

    #[test]
    fn source_caps_refresh_cancels_in_flight_contract_when_it_is_no_longer_supported() {
        let old_contract = TestContract { voltage_mv: 9_000 };
        let new_contract = TestContract { voltage_mv: 15_000 };

        let mut tracker = ContractTracker::default();
        tracker.begin_request(old_contract);
        assert!(tracker.mark_accept_received());
        assert_eq!(tracker.commit_pending_contract(), Some(old_contract));

        tracker.begin_request(new_contract);
        assert_eq!(tracker.pending_contract(), Some(new_contract));
        assert!(tracker.waiting_for_accept());
        tracker.refresh_source_capabilities(false);

        assert_eq!(tracker.active_contract(), Some(old_contract));
        assert_eq!(tracker.pending_contract(), None);
        assert!(!tracker.request_in_flight());
    }

    #[test]
    fn reject_like_cancellation_keeps_existing_contract_active() {
        let old_contract = TestContract { voltage_mv: 12_000 };
        let new_contract = TestContract { voltage_mv: 20_000 };

        let mut tracker = ContractTracker::default();
        tracker.begin_request(old_contract);
        assert!(tracker.mark_accept_received());
        assert_eq!(tracker.commit_pending_contract(), Some(old_contract));

        tracker.begin_request(new_contract);
        tracker.cancel_pending_request();

        assert_eq!(tracker.active_contract(), Some(old_contract));
        assert_eq!(tracker.pending_contract(), None);
        assert!(!tracker.waiting_for_accept());
        assert!(!tracker.waiting_for_ps_rdy());
    }
}
