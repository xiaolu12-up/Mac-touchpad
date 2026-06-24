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
        if contacts.is_empty() {
            return ContactResult::Ignored;
        }

        let contacts_len = contacts.len() as u32;

        // Case 1: Contact count matches contacts received — complete report
        if count > 0 && count == contacts_len {
            self.last_contacts.clear();
            self.target_contact_count = 0;
            return ContactResult::Complete(contacts);
        }

        // Case 2: Contact count is 0 (not reported) — treat as complete
        if count == 0 {
            if !self.last_contacts.is_empty() {
                self.last_contacts.clear();
                self.target_contact_count = 0;
            }
            return ContactResult::Complete(contacts);
        }

        // Case 3: count > contacts_len — partial report, need more data
        if count > contacts_len {
            if self.last_contacts.is_empty() {
                // First partial report
                self.target_contact_count = count;
                self.last_contacts = contacts;
                return ContactResult::Pending;
            }

            // Continuation — merge by contact ID
            for contact in contacts {
                if let Some(existing) = self
                    .last_contacts
                    .iter_mut()
                    .find(|c| c.contact_id == contact.contact_id)
                {
                    *existing = contact;
                } else {
                    self.last_contacts.push(contact);
                }
            }

            if self.last_contacts.len() as u32 >= self.target_contact_count {
                let result = std::mem::take(&mut self.last_contacts);
                self.target_contact_count = 0;
                return ContactResult::Complete(result);
            }

            return ContactResult::Pending;
        }

        // Case 4: count < contacts_len — received more than expected, clamp
        let mut clamped = contacts;
        clamped.truncate(count as usize);
        self.last_contacts.clear();
        self.target_contact_count = 0;
        ContactResult::Complete(clamped)
    }
}
