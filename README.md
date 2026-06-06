# gwt-sweep

`gwt-sweep` cleans up stale Git worktrees. It is designed to make removal safe by default: the command starts in dry-run mode, protects the current worktree, locked worktrees, and dirty worktrees, and only deletes local branches when explicitly requested and safety checks pass.

## Installation

After the crate is published:

```sh
cargo install gwt-sweep
```

From a local checkout:

```sh
cargo install --path .
```

## Usage

```text
Usage: gwt-sweep [OPTIONS] [PATH]...

Arguments:
  [PATH]...  Git repository paths to inspect

Options:
  -r, --recursive              Recursively discover Git repositories below each path
      --hidden                 Include hidden directories during recursive discovery
      --gone                   Select worktrees whose local branch tracks a gone upstream
      --merged                 Select worktrees whose HEAD is merged into the merge base
      --merged-to <REF>        Select worktrees merged into this ref
      --older-than <DURATION>  Select worktrees whose latest filesystem mtime is older than the duration
      --branch <GLOB>          Keep only worktrees whose branch matches the glob
      --exclude-branch <GLOB>  Exclude worktrees whose branch matches the glob
      --include-path <GLOB>    Keep only worktrees whose path or basename matches the glob
      --exclude-path <GLOB>    Exclude worktrees whose path or basename matches the glob
      --all                    Select all worktrees before safety checks, then apply filters
      --force                  Actually remove matched worktrees
      --force-with-dirty       Allow dirty worktrees to be removed when --force is also set
      --delete-branch          Delete the local branch after a successful worktree removal
      --json                   Print a machine-readable JSON report
      --verbose                Print summaries and detailed errors
  -h, --help                   Print help
  -V, --version                Print version
```

With no selector, `gwt-sweep` looks for worktrees whose branch tracks a gone upstream or whose HEAD is merged into the default merge base. The default merge base is resolved from `origin/HEAD`, `main`, or `master`. Use `--merged-to <REF>` when your repository uses a different trunk branch.

By default, text output is minimal: no output is printed when nothing matches, summaries are omitted, and detailed errors are hidden. Use `--verbose` for summaries and detailed errors, or `--json` for a machine-readable report. A non-zero exit status still indicates that errors were encountered.

## Examples

Preview cleanup candidates for the current repository:

```sh
gwt-sweep
```

Inspect repositories below the current directory, including hidden directories:

```sh
gwt-sweep --recursive --hidden .
```

Show a detailed JSON report:

```sh
gwt-sweep --json
```

Remove worktrees whose upstream branch is gone:

```sh
gwt-sweep --gone --force
```

Remove worktrees merged into `trunk` and delete their local branches when safe:

```sh
gwt-sweep --merged-to trunk --force --delete-branch
```

## Safety

- Dry-run mode is the default. Use `--force` to remove matched worktrees.
- The current worktree is protected unless `--all` is used, and even then safety checks still apply.
- Locked worktrees are skipped.
- Dirty worktrees are skipped unless `--force-with-dirty` is also used.
- Prunable worktree metadata is reported but not removed by sweep.
- Local branches are only deleted with `--delete-branch`, after the worktree is removed, and only when the branch is merged into the selected deletion base.

## 日本語

`gwt-sweep` は、不要になった Git worktree を安全に片付けるための CLI です。デフォルトは dry-run で、現在の worktree、locked worktree、dirty な worktree を保護します。ローカルブランチの削除も、明示的に指定され、安全性チェックを通った場合だけ行います。

## インストール

crate 公開後:

```sh
cargo install gwt-sweep
```

ローカル checkout から:

```sh
cargo install --path .
```

## 使い方

```text
Usage: gwt-sweep [OPTIONS] [PATH]...

Arguments:
  [PATH]...  検査する Git repository path

Options:
  -r, --recursive              指定 path 以下の Git repository を再帰的に探す
      --hidden                 再帰探索時に hidden directory も含める
      --gone                   upstream が消えた branch の worktree を選択する
      --merged                 merge base に merge 済みの worktree を選択する
      --merged-to <REF>        指定 ref に merge 済みの worktree を選択する
      --older-than <DURATION>  最新 mtime が指定 duration より古い worktree を選択する
      --branch <GLOB>          branch 名が glob に一致する worktree だけ残す
      --exclude-branch <GLOB>  branch 名が glob に一致する worktree を除外する
      --include-path <GLOB>    path または basename が glob に一致する worktree だけ残す
      --exclude-path <GLOB>    path または basename が glob に一致する worktree を除外する
      --all                    安全性チェック前の候補として全 worktree を選択する
      --force                  一致した worktree を実際に削除する
      --force-with-dirty       --force と併用して dirty な worktree の削除を許可する
      --delete-branch          worktree 削除後、安全な場合にローカル branch も削除する
      --json                   機械可読な JSON report を出力する
      --verbose                summary と詳細エラーを出力する
  -h, --help                   help を表示する
  -V, --version                version を表示する
```

selector を指定しない場合、`gwt-sweep` は upstream が消えた branch の worktree と、デフォルトの merge base に merge 済みの worktree を探します。デフォルトの merge base は `origin/HEAD`、`main`、`master` から解決します。`trunk` など別の branch を使う repository では `--merged-to <REF>` を指定してください。

デフォルトの通常出力は最小限です。対象がなければ何も出力せず、summary や詳細エラーも出しません。summary と詳細エラーが必要な場合は `--verbose`、機械可読な report が必要な場合は `--json` を使ってください。エラーが発生した場合は、通常出力に表示されなくても exit status は non-zero になります。

## 例

現在の repository で削除候補を確認する:

```sh
gwt-sweep
```

現在の directory 以下を再帰的に探し、hidden directory も含める:

```sh
gwt-sweep --recursive --hidden .
```

詳細な JSON report を出力する:

```sh
gwt-sweep --json
```

upstream が消えた branch の worktree を削除する:

```sh
gwt-sweep --gone --force
```

`trunk` に merge 済みの worktree を削除し、安全な場合はローカル branch も削除する:

```sh
gwt-sweep --merged-to trunk --force --delete-branch
```

## 安全性

- デフォルトは dry-run です。実際に削除するには `--force` を指定します。
- 現在の worktree は `--all` なしでは保護されます。`--all` を指定しても安全性チェックは残ります。
- locked worktree は skip します。
- dirty な worktree は `--force-with-dirty` なしでは skip します。
- prunable worktree metadata は報告しますが、sweep では削除しません。
- ローカル branch は `--delete-branch` 指定時のみ削除します。worktree 削除後、選択された deletion base に merge 済みであることを確認してから削除します。

## License

MIT
