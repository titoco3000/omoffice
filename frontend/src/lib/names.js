//! One method for turning a user id into a name to show, reused everywhere
//! a person appears by id alone (chat senders, meeting attendees, invite
//! senders) rather than each place picking its own fallback chain.
//!
//! The precedence itself -- org name, else global name, else email, else
//! the id -- is already baked into each roster entry's own `display_name`
//! by `core::rbac::display_name` (backend/lib/core/rbac/mod.rs), computed
//! against whichever org the roster (see `useOrgRoster`) was fetched for.
//! This is just the lookup by id against that roster, for a caller that
//! only has the id.

export function preferredName(userId, members) {
  return (
    members?.find((member) => member.user_id === userId)?.display_name ?? userId
  );
}
