#!/usr/bin/env bash
# Every commit in a range carries a Signed-off-by trailer for its author.
#
# Why a script and not an action: the check is eleven lines of git, and every
# third-party action in this repository is one more thing to pin, audit and
# read release notes for. The DCO bot's rules are followed: merge commits are
# exempt, and the trailer must name the author or the committer, so a sign-off
# copied from somebody else's commit does not count.
#
#   .github/scripts/dco-check.sh <base sha> <head sha>
set -euo pipefail

base="$1"
head="$2"
bad=0

while IFS= read -r sha; do
  [ -z "$sha" ] && continue
  author="$(git log -1 --format='%ae' "$sha")"
  committer="$(git log -1 --format='%ce' "$sha")"
  trailers="$(git log -1 --format='%(trailers:key=Signed-off-by,valueonly)' "$sha")"
  if grep -qiF "<${author}>" <<<"$trailers" || grep -qiF "<${committer}>" <<<"$trailers"; then
    continue
  fi
  subject="$(git log -1 --format='%s' "$sha")"
  echo "::error::Commit ${sha:0:12} (\"${subject}\") has no Signed-off-by trailer for ${author}. Sign it with \`git commit --amend -s\`, or sign the whole branch with \`git rebase --signoff ${base:0:12}\`, then push again. CONTRIBUTING.md says what the trailer means."
  bad=1
done < <(git rev-list --no-merges "${base}..${head}")

if [ "$bad" -ne 0 ]; then
  exit 1
fi
echo "Every commit in ${base:0:12}..${head:0:12} is signed off."
