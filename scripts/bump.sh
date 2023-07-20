#!/usr/bin/env bash

last_tag=$(git describe --tags --abbrev=0)
commit_subjects=$(git log --pretty=format:"%s" --no-merges  "$last_tag"..)
commit_bodies=$(git log --pretty=format:"%b" --no-merges  "$last_tag"..)

patch=false
minor=false
major=false
while IFS= read -r line; do
  if [[ "$line" =~ ^(fix)|(perf)|(build)|(docs\(readme\)): ]]; then
    patch=true
  elif [[ "$line" =~ ^feat: ]]; then
    minor=true
  elif [[ "$line" =~ ^.*!: ]]; then
    major=true
  fi
done <<< "$commit_subjects"

if [[ $commit_bodies =~ BREAKING\ CHANGE: ]]; then
  major=true
fi

bump=""
if [[ $major == true ]]; then
  bump="major"
elif [[ $minor == true ]]; then
  bump="minor"
elif [[ $patch == true ]]; then
  bump="patch"
fi

if [[ $bump ]]; then
  cargo ws version -a --force '*' -y --no-git-push --no-individual-tags  $bump
  git-cliff -o CHANGELOG.md
  git add CHANGELOG.md
  new_tag=$(git describe --tags --abbrev=0)
  git tag -d "$new_tag"
  git commit --amend --no-edit
  git tag "$new_tag"
  git push
  git push --tags
  exit 0
else
  exit 1
fi