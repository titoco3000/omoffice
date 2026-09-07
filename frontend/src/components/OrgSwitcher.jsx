import { useOrgContext } from "../org/OrgContext";

/** The one place a person picks which org they're currently acting in. */
export default function OrgSwitcher() {
  const { orgs, selectedOrgId, selectOrg } = useOrgContext();

  if (orgs.length === 0) return null;

  return (
    <select
      value={selectedOrgId ?? ""}
      onChange={(event) => selectOrg(event.target.value)}
      className="h-9 rounded-lg border border-slate-700 bg-slate-950 px-2 text-sm text-slate-100 outline-none focus:border-sky-500"
      title="the org chat and meetings act in"
    >
      {orgs.map((org) => (
        <option key={org.id} value={org.id}>
          {org.name}
        </option>
      ))}
    </select>
  );
}
