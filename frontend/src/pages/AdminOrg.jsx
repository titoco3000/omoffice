import { AdminGate, AdminOrgView } from "../features/admin";

export default function AdminOrg({ orgId }) {
  return (
    <AdminGate>
      <AdminOrgView orgId={orgId} />
    </AdminGate>
  );
}
