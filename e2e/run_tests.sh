#!/bin/bash

set -e

SCRIPT_DIR=$(dirname $(readlink -f $0))

python -m venv /tmp/qarax-e2e
source /tmp/qarax-e2e/bin/activate
pip install -r ${SCRIPT_DIR}/requirements.txt
PYTHONPATH=${SCRIPT_DIR} pytest -svv ${SCRIPT_DIR}/e2e.py
deactivate
