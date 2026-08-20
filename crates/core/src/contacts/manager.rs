use crate::hid::types::TouchpadContact;

/// Manages partial contact report reassembly from HID reports.
///
/// Some touchpads send contacts in split/partial reports (fewer contacts
/// than `contact_count` indicates). This manager accumulates partial reports
/// until the full set arrives.
pub struct ContactsManager {
    /// Accumulated contacts from partial reports.
    last_contacts: Vec<TouchpadContact>,
    /// Expected total contact count from the first partial report.
    target_contact_count: u32,
}

/// Result of processing a contact report.
pub enum ContactResult {
    /// A complete set of contacts ready for gesture processing.
    Complete(Vec<TouchpadContact>),
    /// Still accumulating partial reports.
    Pending,
    /// Empty or invalid report, ignored.
    Ignored,
}

impl ContactsManager {
    pub fn new() -> Self {
        Self {
            last_contacts: Vec::new(),
            target_contact_count: 0,
        }
    }

    /// Process incoming touchpad contacts.
    ///
    /// `contacts`: the contacts parsed from this HID report.
    /// `count`: the contact count reported by the touchpad.
    pub fn receive(
        &mut self,
        contacts: Vec<TouchpadContact>,
        count: u32,
    ) -> ContactResult {
        // Empty contacts (all fingers lifted) — always pass through to notify engine
        if contacts.is_empty() {
            self.last_contacts.clear();
            self.target_contact_count = 0;
            return ContactResult::Complete(contacts);
        }

        let contacts_len = contacts.len() as u32;

        // Regular contact list (count matches contacts received)
        if count == contacts_len {
            self.last_contacts.clear();
            self.target_contact_count = 0;
            return ContactResult::Complete(contacts);
        }

        // Partial contact list continuation (count == 0, sent after an incomplete contact list)
        if count == 0 {
            self.last_contacts.extend(contacts);
            remove_duplicates(&mut self.last_contacts);

            if self.target_contact_count == 0 {
                return ContactResult::Pending;
            }

            if self.last_contacts.len() as u32 >= self.target_contact_count {
                self.last_contacts.truncate(self.target_contact_count as usize);
                let result = std::mem::take(&mut self.last_contacts);
                self.target_contact_count = 0;
                return ContactResult::Complete(result);
            }

            return ContactResult::Pending;
        }

        // Old partial list was not completed, but a new non-zero count report arrived
        if !self.last_contacts.is_empty() {
            self.last_contacts.clear();
            self.target_contact_count = 0;
        }

        // Regular contact list with more contacts than expected: clamp
        if count <= contacts_len {
            let mut clamped = contacts;
            clamped.truncate(count as usize);
            self.last_contacts.clear();
            self.target_contact_count = 0;
            return ContactResult::Complete(clamped);
        }

        // Incomplete contact list: 0 < contacts.len() < count
        self.target_contact_count = count;
        self.last_contacts = contacts;
        ContactResult::Pending
    }
}

fn remove_duplicates(contacts: &mut Vec<TouchpadContact>) {
    let mut unique = Vec::with_capacity(contacts.len());
    for c in contacts.drain(..) {
        if !unique.iter().any(|existing: &TouchpadContact| existing.contact_id == c.contact_id) {
            unique.push(c);
        }
    }
    *contacts = unique;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contacts_manager_regular() {
        let mut mgr = ContactsManager::new();
        let c1 = TouchpadContact::new(1, 100, 200);
        let c2 = TouchpadContact::new(2, 300, 400);
        let res = mgr.receive(vec![c1, c2], 2);
        match res {
            ContactResult::Complete(list) => assert_eq!(list.len(), 2),
            _ => panic!("Expected Complete"),
        }
    }

    #[test]
    fn test_contacts_manager_partial_reassembly() {
        let mut mgr = ContactsManager::new();
        let c1 = TouchpadContact::new(1, 100, 200);
        let c2 = TouchpadContact::new(2, 300, 400);
        let c3 = TouchpadContact::new(3, 500, 600);

        // 1st report: count = 3, but only 1 contact
        let res1 = mgr.receive(vec![c1], 3);
        assert!(matches!(res1, ContactResult::Pending));

        // 2nd report: count = 0, contact 2
        let res2 = mgr.receive(vec![c2], 0);
        assert!(matches!(res2, ContactResult::Pending));

        // 3rd report: count = 0, contact 3
        let res3 = mgr.receive(vec![c3], 0);
        match res3 {
            ContactResult::Complete(list) => {
                assert_eq!(list.len(), 3);
                assert_eq!(list[0].contact_id, 1);
                assert_eq!(list[1].contact_id, 2);
                assert_eq!(list[2].contact_id, 3);
            }
            _ => panic!("Expected Complete on 3rd report"),
        }
    }
}
