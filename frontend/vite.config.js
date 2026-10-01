import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Compiles `*.csv` imports into a JS array of row objects at build time, so
// the data ships as plain literals and nothing is parsed in the browser.
// Header names become keys; a value that parses as a number becomes one, and
// "true"/"false" become booleans. RFC 4180 subset: `"` quotes a field (`""`
// for a literal quote).
function csvPlugin() {
  const parse = (text, file) => {
    const rows = [];
    let row = [];
    let field = "";
    let quoted = false;
    for (let i = 0; i < text.length; i++) {
      const c = text[i];
      if (c === '"' && quoted && text[i + 1] === '"') {
        field += '"';
        i++;
      } else if (c === '"') {
        quoted = !quoted;
      } else if (c === "," && !quoted) {
        row.push(field);
        field = "";
      } else if (c === "\r" && !quoted) {
        // CRLF: the \n ends the record
      } else if (c === "\n" && !quoted) {
        row.push(field);
        field = "";
        rows.push(row);
        row = [];
      } else {
        field += c;
      }
    }
    if (quoted) throw new Error(`${file}: unterminated quote`);
    if (field || row.length) rows.push([...row, field]);
    return rows;
  };
  const typed = (v) =>
    v === "true"
      ? true
      : v === "false"
        ? false
        : v !== "" && !isNaN(v)
          ? Number(v)
          : v;

  return {
    name: "csv",
    transform(code, id) {
      if (!id.endsWith(".csv")) return null;
      const [header, ...rows] = parse(code, id);
      const objects = rows.map((row, i) => {
        if (row.length !== header.length) {
          throw new Error(`${id}:${i + 2}: expected ${header.length} columns`);
        }
        return Object.fromEntries(header.map((h, j) => [h, typed(row[j])]));
      });
      return { code: `export default ${JSON.stringify(objects)};`, map: null };
    },
  };
}

// The API base URL is not read from a .env file: the Makefile sources
// .env.<stage> and exports VITE_API_URL, exactly like backend/Makefile does.
// That keeps `make run local` from colliding with Vite's own `.env.local`,
// which Vite loads in every mode.
export default defineConfig({
  plugins: [react(), tailwindcss(), csvPlugin()],
  // amazon-chime-sdk-js tests for browser APIs with node's `global`
  // (`'document' in global`), which no bundler supplies in the browser; without
  // this, constructing a meeting session throws "global is not defined".
  // Both spellings are needed: `define` covers the production build, the
  // optimizeDeps one covers the dependency esbuild pre-bundles for `vite dev`.
  define: { global: "globalThis" },
  optimizeDeps: {
    esbuildOptions: { define: { global: "globalThis" } },
  },
  server: {
    port: Number(process.env.PORT ?? 5175),
    strictPort: true,
  },
});
