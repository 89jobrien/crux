#!/usr/bin/env nu
# Scans ~/dev workspace, writes input JSON, and runs session_handoff.crux.
# Usage: nu examples/joe/session_handoff_scan.nu

let dev = $"($env.HOME)/dev"

let dirty_repos = (ls $dev
  | where type == dir
  | each { |d|
    let git_dir = ($d.name | path join ".git")
    if ($git_dir | path exists) {
      let status = (do { git -C $d.name status --porcelain } | complete)
      if $status.exit_code == 0 and ($status.stdout | str trim | str length) > 0 {
        $d.name | path basename
      }
    }
  }
  | compact)

let handoffs = (glob $"($dev)/**/.ctx/HANDOFF*.yaml" --depth 3
  | where {|f|
    let last_session = $"($dev)/.ctx/last-session"
    if ($last_session | path exists) {
      (ls $f | first | get modified) > (ls $last_session | first | get modified)
    } else {
      true
    }
  })

let pending_todos = (do { doob todo list --status pending --format json } | complete
  | if $in.exit_code == 0 { $in.stdout | from json } else { [] })

let stale_worktrees = (ls $dev
  | where type == dir
  | each { |d|
    let git_dir = ($d.name | path join ".git")
    if ($git_dir | path exists) {
      let wt = (do { git -C $d.name worktree list } | complete)
      if $wt.exit_code == 0 {
        $wt.stdout | lines | skip 1 | each { |line|
          $"($d.name | path basename): ($line)"
        }
      }
    }
  }
  | compact
  | flatten)

let recent_activity = (ls $dev
  | where type == dir
  | each { |d|
    let git_dir = ($d.name | path join ".git")
    if ($git_dir | path exists) {
      let log = (do { git -C $d.name log --since="24 hours ago" --oneline -1 } | complete)
      if $log.exit_code == 0 and ($log.stdout | str trim | str length) > 0 {
        $"($d.name | path basename): ($log.stdout | str trim)"
      }
    }
  }
  | compact)

{
  dirty_repos: $dirty_repos,
  handoffs: $handoffs,
  pending_todos: $pending_todos,
  stale_worktrees: $stale_worktrees,
  recent_activity: $recent_activity,
} | to json | save -f /tmp/session_handoff_input.json

crux run examples/joe/session_handoff.crux /tmp/session_handoff_input.json
