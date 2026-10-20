## AgentKnife fixed UI strings (English).
## Every message here must also exist in zh-TW/ak.ftl (checked by `cargo xtask ci`).

app-name = AgentKnife

## Common actions

common-ok = OK
common-cancel = Cancel
common-continue = Continue
common-start = Start
common-details = Details
common-retry = Try again

## Doctor progress (deterministic steps)

# $tool (String) tool being installed; $step / $total (Number) progress
doctor-installing = Installing { $tool } ({ $step }/{ $total })…
doctor-ready = Everything you need is installed.

## Agent step confirmations ($detail: the command or path, shown verbatim)

confirm-download = This step downloads or installs software from the internet: { $detail }
confirm-delete = This step deletes files or throws away changes: { $detail }
confirm-outside-project = This step touches files outside your project folder: { $detail }
confirm-elevated = This step asks for administrator rights: { $detail }
confirm-unrecognized = AgentKnife cannot tell what this step will do: { $detail }
confirm-change = You asked to approve every change. Allow this one? { $detail }
denied-path-escape = Blocked: this is outside your project folder: { $detail }
denied-protected-path = Blocked: this is AgentKnife's own history and must not be edited by hand: { $detail }
denied-dangerous = Blocked: this could damage your computer: { $detail }

## Timeline entries (checkpoints)

timeline-initial = Starting point
timeline-turn = Change
timeline-task = Task finished
timeline-bundle = Installer built
timeline-marked = Marked version
timeline-abandoned = Set aside (can be brought back)
timeline-restore = Went back to an earlier version
