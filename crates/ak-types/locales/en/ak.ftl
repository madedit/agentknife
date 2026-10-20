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
