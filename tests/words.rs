//! The screen's words (`docs/api.md`, 「The console plugin bundle」, the wording): the language pack holds the
//! bundle's namespace alone, and every key the screen's code writes whole is either the pack's or one of the
//! layer's own keys, as the `wording-keys` module of the release the client vendors lists them, so a console
//! that answers the layer's keys and merges the bundle's words shows no key raw.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

/// Reads every `.ts` and `.vue` file under the folders the screen is written in, a component as its scripts and
/// its template compiled to the render function it ships as, and answers what breaks the wording: a key under
/// one of the layer's domains its list lacks, a key under the bundle's namespace some locale's pack does not
/// answer, and a pack holding a tree beside that namespace.
const READING: &str = r#"import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { babelParse, compileTemplate, parse } from "vue/compiler-sfc";
import { WORDING_KEYS } from "bmo-ui/shared/wording-keys";

let given = "";
for await (const part of process.stdin) given += String(part);
const { namespace, folders, locales } = JSON.parse(given);

const layerKeys = new Set(WORDING_KEYS);
const layerDomains = new Set(WORDING_KEYS.map((key) => key.split(".")[0]));

function programs(path, source) {
  const program = (code) => babelParse(code, { sourceType: "module", plugins: ["typescript"] }).program;
  if (!path.endsWith(".vue")) return [program(source)];
  const { descriptor } = parse(source, { filename: path });
  const codes = [descriptor.script?.content, descriptor.scriptSetup?.content];
  if (descriptor.template !== null) {
    const compiled = compileTemplate({ source: descriptor.template.content, filename: path, id: path, compilerOptions: { isTS: true } });
    if (compiled.errors.length > 0) throw new Error(`${path}: ${compiled.errors.join("; ")}`);
    codes.push(compiled.code);
  }
  return codes.filter((code) => code !== undefined).map(program);
}

function* strings(node) {
  if (Array.isArray(node)) {
    for (const one of node) yield* strings(one);
    return;
  }
  if (node === null || typeof node !== "object") return;
  if (node.type === "StringLiteral") yield node.value;
  if (node.type === "TemplateLiteral" && node.expressions.length === 0) yield node.quasis[0].value.cooked;
  for (const [field, one] of Object.entries(node)) {
    if (field !== "loc" && field !== "extra") yield* strings(one);
  }
}

function files(folder) {
  return readdirSync(folder, { recursive: true })
    .map(String)
    .filter((path) => /\.(ts|vue)$/.test(path) && !path.endsWith(".d.ts"))
    .map((path) => join(folder, path));
}

const packs = Object.fromEntries(locales.map((path) => [path, JSON.parse(readFileSync(path, "utf8"))]));
const answered = (pack, key) => typeof key.split(".").reduce((tree, part) => tree?.[part], pack) === "string";

const unlisted = [];
const unanswered = [];
for (const path of folders.flatMap(files)) {
  for (const program of programs(path, readFileSync(path, "utf8"))) {
    for (const text of strings(program)) {
      const [domain, ...rest] = text.split(".");
      if (rest.length === 0 || /\s|\//.test(text)) continue;
      if (layerDomains.has(domain) && !layerKeys.has(text)) unlisted.push(`${text} in ${path}`);
      if (domain === namespace) {
        for (const [locale, pack] of Object.entries(packs)) {
          if (!answered(pack, text)) unanswered.push(`${text} in ${path}, not in ${locale}`);
        }
      }
    }
  }
}

const besides = Object.entries(packs).flatMap(([locale, pack]) =>
  Object.keys(pack).filter((tree) => tree !== namespace).map((tree) => `${tree} in ${locale}`)
);

process.stdout.write(JSON.stringify({ unlisted: [...new Set(unlisted)], unanswered: [...new Set(unanswered)], besides }));
"#;

fn client() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("client")
}

/// The bundle's namespace, which is its name in the service's declaration.
fn namespace() -> String {
  let declaration: Value =
    serde_json::from_str(bmo_mail::console::DECLARATION).expect("the declaration is JSON");

  declaration["name"].as_str().expect("the declaration names the bundle").to_owned()
}

/// Every locale's pack under `client/i18n/locales/`.
fn locales() -> Vec<String> {
  std::fs::read_dir(client().join("i18n").join("locales"))
    .expect("the language pack's folder reads")
    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
    .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
    .map(|path| path.display().to_string())
    .collect()
}

/// The reading's faults, run by Bun in the client's project on the bmo-ui release it installed from the vendored
/// file.
fn faults() -> Value {
  let folders: Vec<String> = ["app", "console", "shared"]
    .iter()
    .map(|folder| client().join(folder).display().to_string())
    .collect();
  let asked = serde_json::json!({ "namespace": namespace(), "folders": folders, "locales": locales() });

  let mut bun = Command::new("bun")
    .args(["--eval", READING])
    .current_dir(client())
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("bun starts");

  {
    use std::io::Write;
    let mut stdin = bun.stdin.take().expect("bun's input is open");
    stdin.write_all(asked.to_string().as_bytes()).expect("the question is written");
  }

  let output = bun.wait_with_output().expect("bun ends");

  assert!(output.status.success(), "the reading ran: {}", String::from_utf8_lossy(&output.stderr));

  serde_json::from_slice(&output.stdout).expect("the reading answers JSON")
}

#[test]
fn the_screen_asks_only_its_own_words_and_the_layers() {
  let faults = faults();

  assert_eq!(
    faults["unlisted"],
    serde_json::json!([]),
    "keys under the layer's domains its list lacks"
  );
  assert_eq!(
    faults["unanswered"],
    serde_json::json!([]),
    "keys under the bundle's namespace no pack answers"
  );
  assert_eq!(
    faults["besides"],
    serde_json::json!([]),
    "trees beside the bundle's namespace in the pack"
  );
}

/// Each word a console raises a notice of a declared kind in takes one field of the member the event carries and
/// nothing beside it, so no separator stands between fields of which one may be empty (`docs/api.md`, 「The console
/// plugin bundle」, the wording): a new message's title is its subject and its body its sender.
#[test]
fn a_notice_word_takes_one_field_alone() {
  let declaration: Value =
    serde_json::from_str(bmo_mail::console::DECLARATION).expect("the declaration is JSON");
  let kinds = declaration["pushKinds"].as_array().expect("the declaration names its notice kinds");

  for path in locales() {
    let pack: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("the pack reads"))
      .expect("the pack is JSON");

    for kind in kinds {
      let kind = kind.as_str().expect("a kind is text");
      let mut words = &pack[namespace()]["notice"];

      for part in kind.split('.') {
        words = &words[part];
      }

      for role in ["title", "body"] {
        let word = words[role].as_str().unwrap_or_else(|| panic!("{path} gives {kind} a {role}"));

        assert!(
          word.starts_with('{') && word.ends_with('}') && word.matches('{').count() == 1,
          "{path}: the {role} of {kind} takes one field alone, not {word:?}"
        );
      }
    }
  }

  let created = &serde_json::from_str::<Value>(
    &std::fs::read_to_string(client().join("i18n").join("locales").join("ko.json"))
      .expect("the pack reads"),
  )
  .expect("the pack is JSON")[namespace()]["notice"]["message"]["created"];

  assert_eq!(created["title"], "{subject}");
  assert_eq!(created["body"], "{from}");
}
