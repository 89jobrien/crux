#!/usr/bin/env bash

devloop git timeline | jq '[.entries[] | select(.kind | has("Commit"))][:5]'
