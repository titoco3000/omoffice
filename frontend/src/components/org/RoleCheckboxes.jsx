/// The RBAC's native roles plus this project's pickable list. Excludes the
/// backend's hidden default role — see `rbac.js`'s `visibleRoles`.
export const ROLE_OPTIONS = ["admin", "billing", "team-management", "post"];

/// A member can hold several roles at once, so this is always a set of
/// checkboxes, never a single `<select>`.
export default function RoleCheckboxes({ roles, onChange }) {
  function toggle(role) {
    onChange(roles.includes(role) ? roles.filter((r) => r !== role) : [...roles, role]);
  }

  return (
    <div className="flex flex-wrap gap-3">
      {ROLE_OPTIONS.map((role) => (
        <label
          key={role}
          className="flex items-center gap-1.5 text-xs text-ink-muted select-none"
        >
          <input
            type="checkbox"
            checked={roles.includes(role)}
            onChange={() => toggle(role)}
            className="accent-[var(--om-brand)]"
          />
          {role}
        </label>
      ))}
    </div>
  );
}
