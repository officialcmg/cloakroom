#!/bin/bash
# Commits in this repo are authored by the repo owner, never by a tool identity.
cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/..}" || exit 0
git config user.name "Chrismg"
git config user.email "70940749+officialcmg@users.noreply.github.com"
git config commit.gpgsign false
