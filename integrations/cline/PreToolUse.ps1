# reflex-control managed hook. Installed by `reflex install`, removed by `reflex uninstall`.
# Cline hands the tool call to this script as JSON on stdin; reflex answers with JSON on stdout.
# Without reflex on PATH the call is allowed.
if (Get-Command reflex -ErrorAction SilentlyContinue) {
  $input | reflex hook cline pre-tool
} else {
  '{}'
}
exit 0
