#!/usr/bin/env bash
# Run every acceptance wrapper in this directory, surface a final
# summary. Each scenario runs in isolation (full teardown between).
#
# Exit code is the count of failed scenarios (0 = all pass).

set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

declare -a passed failed

for wrapper in *.acceptance.sh; do
    [ -f "$wrapper" ] || continue
    name="${wrapper%.acceptance.sh}"
    echo
    echo "════════════════════════════════════════════════"
    echo " Running $name"
    echo "════════════════════════════════════════════════"
    if "./$wrapper"; then
        passed+=("$name")
    else
        failed+=("$name")
    fi
done

echo
echo "════════════════════════════════════════════════"
echo " Summary"
echo "════════════════════════════════════════════════"
printf 'passed (%d): %s\n' "${#passed[@]}" "${passed[*]:-—}"
printf 'failed (%d): %s\n' "${#failed[@]}" "${failed[*]:-—}"

exit "${#failed[@]}"
