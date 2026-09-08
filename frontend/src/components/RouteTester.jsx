import { useMemo, useState } from "react";
import { buildPath, callApi, pathParams } from "../api";

/**
 * One API route, with a form for whatever it needs. `{placeholders}` become
 * path fields, `fields` becomes the JSON body, `onResult` gets the response.
 */
export default function RouteTester({
  route,
  method = "GET",
  fields,
  token,
  onResult,
  note,
}) {
  const params = useMemo(() => pathParams(route), [route]);
  const bodyFields = useMemo(() => Object.entries(fields ?? {}), [fields]);

  const [paramValues, setParamValues] = useState(() =>
    Object.fromEntries(params.map((name) => [name, ""])),
  );
  const [bodyValues, setBodyValues] = useState(() =>
    Object.fromEntries(bodyFields.map(([name, type]) => [name, blank(type)])),
  );
  const [result, setResult] = useState(null);
  const [pending, setPending] = useState(false);

  const needsToken = route.startsWith("/private");
  const path = buildPath(route, paramValues);

  async function submit(event) {
    event.preventDefault();
    setPending(true);
    const response = await callApi({
      method,
      path,
      token,
      body: bodyFields.length ? coerce(bodyValues, fields) : undefined,
    });
    setResult(response);
    setPending(false);
    if (response.ok && onResult) onResult(response.data);
  }

  return (
    <form
      onSubmit={submit}
      className="rounded-card border border-line bg-muted p-4"
    >
      <div className="flex flex-wrap items-center gap-2">
        <span
          className={`rounded px-2 py-0.5 font-mono text-xs font-bold ${
            method === "GET"
              ? "bg-success/15 text-success"
              : "bg-warning/20 text-warning"
          }`}
        >
          {method}
        </span>
        <code className="font-mono text-sm text-ink">{route}</code>
        {needsToken && (
          <span
            className={`rounded px-2 py-0.5 text-xs ${
              token ? "bg-brand/15 text-brand" : "bg-danger/15 text-danger"
            }`}
            title={
              token ? "sending the current token" : "no token held yet"
            }
          >
            {token ? "token" : "no token"}
          </span>
        )}
      </div>

      {note && <p className="mt-2 text-xs text-ink-faint">{note}</p>}

      {(params.length > 0 || bodyFields.length > 0) && (
        <div className="mt-3 grid gap-3 sm:grid-cols-2">
          {params.map((name) => (
            <Field key={`path-${name}`} label={name} hint="path">
              <input
                type="text"
                value={paramValues[name]}
                onChange={(event) =>
                  setParamValues((current) => ({
                    ...current,
                    [name]: event.target.value,
                  }))
                }
                placeholder={name}
                className={inputClass}
              />
            </Field>
          ))}

          {bodyFields.map(([name, type]) => (
            <Field key={`body-${name}`} label={name} hint={type}>
              {type === "boolean" ? (
                <label className="flex h-9 items-center gap-2 text-sm text-ink-muted">
                  <input
                    type="checkbox"
                    checked={Boolean(bodyValues[name])}
                    onChange={(event) =>
                      setBodyValues((current) => ({
                        ...current,
                        [name]: event.target.checked,
                      }))
                    }
                    className="h-4 w-4 accent-[var(--om-brand)]"
                  />
                  {String(Boolean(bodyValues[name]))}
                </label>
              ) : (
                <input
                  type={type === "number" ? "number" : "text"}
                  value={bodyValues[name]}
                  onChange={(event) =>
                    setBodyValues((current) => ({
                      ...current,
                      [name]: event.target.value,
                    }))
                  }
                  placeholder={name}
                  className={inputClass}
                />
              )}
            </Field>
          ))}
        </div>
      )}

      <div className="mt-3 flex items-center gap-3">
        <button
          type="submit"
          disabled={pending}
          className="rounded-lg bg-brand px-4 py-1.5 text-sm font-medium text-on-brand hover:bg-brand-strong disabled:opacity-50"
        >
          {pending ? "sending..." : "send"}
        </button>
        <code className="min-w-0 flex-1 truncate font-mono text-xs text-ink-faint">
          {method} {path}
        </code>
      </div>

      {result && <Result result={result} />}
    </form>
  );
}

const inputClass =
  "h-9 w-full rounded-lg border border-line bg-surface px-2 font-mono text-sm text-ink outline-none focus:border-brand";

function Field({ label, hint, children }) {
  return (
    <label className="block">
      <span className="mb-1 block font-mono text-xs text-ink-muted">
        {label} <span className="text-ink-faint">({hint})</span>
      </span>
      {children}
    </label>
  );
}

function Result({ result }) {
  const tone = result.ok
    ? "border-success/40 text-success"
    : "border-danger/40 text-danger";

  return (
    <div className="mt-3">
      <div className={`flex gap-3 border-b pb-1 font-mono text-xs ${tone}`}>
        <span>{result.status === 0 ? "no response" : result.status}</span>
        <span className="text-ink-faint">{result.ms}ms</span>
      </div>
      <pre className="mt-2 max-h-64 overflow-y-auto rounded-lg bg-surface p-3 font-mono text-xs whitespace-pre-wrap break-all text-ink-muted">
        {JSON.stringify(result.data, null, 2)}
      </pre>
    </div>
  );
}

function blank(type) {
  if (type === "boolean") return false;
  return "";
}

/// Inputs are strings; the API expects the declared types.
function coerce(values, types) {
  return Object.fromEntries(
    Object.entries(values).map(([name, value]) => {
      if (types[name] === "boolean") return [name, Boolean(value)];
      if (types[name] === "number") return [name, Number(value)];
      return [name, value];
    }),
  );
}
