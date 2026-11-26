<!-- nix-tops:begin -->
# AGENTS.md — Nix-TOPS Agent Contract

This section is the contract between this repository and any AI coding agents (or human reviewers) working on its Nix code. `nixtops init` replaces only the text between the `nix-tops` markers.

**Facets:** `dev-flake, package-set`
**Protocol:** Nix-TOPS 0.2.0
**Flake:** `.`
**Systems:** `aarch64-darwin, aarch64-linux, x86_64-linux`
**Enforce:** `ratchet`

## Commands

```bash
nix run .#nixtops -- check          # validate nix-tops.toml
nix run .#nixtops -- gate           # the merge gate (tier ≤ 1): the definition of done
nix run .#nixtops -- laws run       # laws only
nix run .#nixtops -- outputs        # every output and its coverage level
nix fmt                             # format (nixfmt via nixfmt-tree)
```

## Floors (these facets)

- Every output evaluates on every declared system, with warnings fatal and import-from-derivation off
- Output coverage: `100%` built, `80%` tested
- Closure budget: `ratchet` on `every output`
- Under `ratchet`: new outputs meet the floors, and nothing gets worse than `.nix-tops/baseline.json` (per-file findings, closure bytes, eval counters, acks)
- Other layers: see `nix-tops.toml`

## Policies (non-negotiable)

<!-- rules:begin -->
<!-- generated from NIX_TOPS.md section 3.4 by scripts/rules_gen.py; edit the source, not this copy -->

1. **Oracle first.** Write the oracle from the contract before the code. A test that asserts what a module currently produces freezes its bugs. **[review, N-39]**
2. **Pure or nothing.** The gate never passes `--impure`. A change that needs `--impure` to evaluate is not done. **[gate 3]**
3. **Do not weaken a law**, a linter setting, an eval option or a budget so a check passes. Fix the code, or change the law through a `protocol-change` PR (§13). **[`law-diff`, §11]**
4. **Never fabricate a hash.** Use `lib.fakeHash`, build, and copy the `got:` value. A placeholder is never committed. **[N-25]**
5. **Every ack names its law and a reason** (`# nixtops-ack N-NN: reason`). Never edit `.nix-tops/baseline.json` by hand, and never wrap old findings in acks to zero a count. **[§3.3, §6.8]**
6. **No silent empties.** A flake with no checks, an eval that enumerated no outputs, or a report with no layers fails the gate. `not-evaluated` and an unmapped layer are never a pass. **[§3.3]**
7. **No dry gates in CI.** `--dry-run` is refused when `CI=true`. **[§12]**
8. **One gate.** `nixtops gate` is the verification command and the definition of done. Running `nix flake check` alone is not done. **[§11]**
9. **Do not edit generated files** (`hardware-configuration.nix`, `flake.lock` by hand, any file a generator writes). Regenerate them, or fix the generator. **[review]**
10. **Agent-authored code meets the same gate as human code.** Prefer deleting code to adding configuration, and add a dependency only if it deletes code or buys measured safety or performance. **[review]**
11. **Schema precedence.** When prose and schema disagree, fix the schema first, then the prose. **[review]**
<!-- rules:end -->

## Standards, with the check behind each

**[review]** means no tool catches it; the reviewer must.

### Purity and flakes
- **Stage new files before you evaluate.** Flakes copy only files git tracks. Run `git add` on every new `.nix` file before `nix build`, `nix eval` or `nix flake check`, and commit it before CI: a staged file passes locally and fails there. **[N-04]**
- **No `import <nixpkgs> {}`, no `NIX_PATH`.** Take `pkgs` from the flake input: `nixpkgs.legacyPackages.${system}`, or one `import nixpkgs { inherit system; }` per system. Modules use their `pkgs` argument. **[N-01, N-09]**
- **No `builtins.getEnv`, `currentSystem` or `currentTime`.** In pure evaluation `getEnv` returns `""` without an error, so the mistake hides. Pass values through options or `specialArgs`. **[N-02]**
- **No host paths.** No `/home/…` or `/Users/…`, as path literals or strings. Use paths relative to the file. **[N-03]**
- **Every output works on every declared system.** Use the per-system function; never hardcode `"x86_64-linux"` inside it. **[N-08]**
- **No import-from-derivation.** If an output truly needs it, ack it. **[N-07]**
- **Keep one nixpkgs.** Other inputs `follows = "nixpkgs"`. The `nix-tops` input never follows: its tools stay on the kit's pin. **[N-06, N-43]**

### Language
- **No file-scope `with`.** `with pkgs; [ git curl jq ]` around a list is fine; `with pkgs; with lib;` over a whole module is not. Write `lib.mkIf`, `lib.mkDefault`. **[N-10]**
- **No `rec` for derivations.** Use `stdenv.mkDerivation (finalAttrs: { … })` and refer to `finalAttrs.version`. **[N-11]**
- **Verify every `lib` and `builtins` name** against the pinned inputs before you use it: `nix eval --inputs-from . nixpkgs#lib --apply 'lib: lib ? splitString'` (pure; `--inputs-from .` uses this flake's locked nixpkgs). `builtins.stringSplit` and `lib.isEmpty` do not exist. **[N-15]**
- **Predicate first:** `lib.mkIf cond { … }`, `lib.optionalAttrs cond { … }`, `lib.optionals cond [ … ]`. **[review]**
- **No dead bindings; statix and nixfmt clean.** Run `nix fmt`. **[N-12, N-13, N-14]**
- **No `builtins.trace` or `lib.debug` left in.** **[N-16]**

### Options and modules
- **Never guess an option or package name.** Evaluate the configuration: `nix eval .#nixosConfigurations.<host>.config.system.build.toplevel.drvPath`. The module system rejects options that do not exist. Look up options with `nix eval .#nixosConfigurations.<host>.options.<path>.description`. **[N-18]**
- **Fix warnings; do not ignore them.** A renamed option (for example `programs.carapace.ignoreCase`, renamed to `programs.carapace.environment.CARAPACE_MATCH`) is a warning, and the gate refuses every warning no dated `[[warnings.allow]]` entry covers. **[N-17]**
- **Guard module config with `lib.mkIf cfg.enable`.** Importing a module without enabling it must change nothing. **[N-20]**
- **Give every option a `type` and a `description`.** `types.anything` needs an ack. **[N-19]**
- **`lib.mkForce` needs an ack** that names what it overrides. **[N-21]**
- **Never change `system.stateVersion` or `home.stateVersion`** to "upgrade". It is the state format the machine was installed with, not the release. **[N-22]**
- **No blanket `allowUnfree = true`.** Use `allowUnfreePredicate` with a named list. **[N-23]**

### Derivations
- **Never invent a hash.** Set `hash = lib.fakeHash;` (or `vendorHash`, `cargoHash`, `npmDepsHash`), build, and paste the `got:` value. Do not commit the placeholder. **[N-25]**
- **Change the hash when you change the source.** After bumping `version`, `rev`, `tag` or `url`, reset the hash to `lib.fakeHash` and rebuild. A stale hash silently builds the old source, because the old store path still matches. **[N-26]**
- **No network in build phases.** Use `buildGoModule`, `rustPlatform.buildRustPackage`, `buildNpmPackage` or `python3Packages.buildPythonApplication`, never `go build` or `npm install` in a raw `mkDerivation`. A build that works on macOS can still fail on Linux: the macOS sandbox is off by default. **[N-27]**
- **`.override` for function arguments** (`openssl = libressl`, `withPulseAudio = true`); **`.overrideAttrs` for derivation attributes** (`src`, `version`, `patches`, phases, inputs). Append with `(old.patches or [ ]) ++ [ … ]`. **[N-28]**
- **Keep string context.** Interpolate packages or use `lib.getExe` / `lib.getExe'`; never `builtins.unsafeDiscardStringContext` or `/nix/store/…` literals. **[N-29]**
- **Set `meta`:** `description`, `license`, `platforms`, and `mainProgram` for a single-binary package. **[N-30]**
- **Filter `src`** with `lib.fileset.toSource` instead of `./.`. **[N-31]**
- **Use `writeShellApplication`** with `runtimeInputs` for scripts. **[N-32]**

### Secrets and security
- **Nothing secret in a `.nix` file**, in `pkgs.writeText`, `environment.etc` or a systemd `Environment=` string. The store is world-readable. **[N-33]**
- **Secret files are runtime paths:** `config.sops.secrets.<name>.path`, `config.age.secrets.<name>.path`, or `/run/secrets/…`. Never `./secret.env`: a path literal copies the file into the store. **[N-34]**
- **No `password` or `initialPassword` in user definitions.** Use `hashedPasswordFile`. **[N-35]**
- **Never disable the sandbox** (`__noChroot`, `sandbox = false`). **[N-36]**
- **Do not turn the firewall off** to fix connectivity. Open the port the service needs, and ack it. **[N-37]**

### Tests
- **Test exported outputs:** nix-unit for `lib` functions, an eval test for each module, a VM test for each service module. **[output coverage]**
- **VM tests wait on conditions** (`wait_for_unit`, `wait_for_open_port`, `wait_until_succeeds`), never `sleep`. **[N-40]**
- **Assert on what the contract says**, not on whatever the module produces today. **[review, N-39]**

## Verification (before finishing any task)

```bash
nix run .#nixtops -- gate
```

It checks tracked files and the lock, lints, evaluates every output on every system with warnings fatal, builds and runs the checks, computes output coverage and budgets, and runs the laws. Running `nix flake check` alone is not done. A green gate is.

---

*Generated by `nixtops init`. Change laws, floors or budgets only through the protocol change process.*
<!-- nix-tops:end -->
