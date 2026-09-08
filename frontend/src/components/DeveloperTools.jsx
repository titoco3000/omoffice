import RouteTester from "./RouteTester";

/// The sample API's own test bench, kept out of the way in the profile's
/// settings rather than on a screen someone uses. This is what `RouteTester`
/// was built for (`dev-docs/phase/1.md`); nothing else depends on it.
export default function DeveloperTools({ token }) {
  return (
    <details className="rounded-card border border-line bg-surface p-4">
      <summary className="cursor-pointer text-sm font-semibold text-ink">
        developer tools
      </summary>
      <div className="mt-4 grid gap-4">
        <RouteTester route="/" />
        <RouteTester route="/health" />
        <RouteTester route="/private/me" token={token} />
        <RouteTester
          method="POST"
          route="/private/sample-post/{var}"
          fields={{ org_id: "string", name: "string", male: "boolean" }}
          token={token}
          note='requires the "post" role in org_id'
        />
        <RouteTester
          method="POST"
          route="/private/meetings"
          fields={{ org_id: "string" }}
          token={token}
        />
        <RouteTester route="/private/meetings?org_id={org_id}" token={token} />
        <RouteTester route="/private/orgs/{org_id}/presence" token={token} />
        <RouteTester
          method="POST"
          route="/private/meetings/{code}/attendees"
          token={token}
        />
      </div>
    </details>
  );
}
