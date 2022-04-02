#!/bin/bash

set -e

if [[ "$OSTYPE" == "darwin"* ]]; then
    SCRIPT_DIR=$(dirname $(greadlink -f $0))
else
    SCRIPT_DIR=$(dirname $(readlink -f $0))
fi

python3 -m venv /tmp/qarax-e2e
source /tmp/qarax-e2e/bin/activate
pip install -r ${SCRIPT_DIR}/requirements.txt

if [ "$1" == "--keep" ]; then
     PYTHONPATH=${SCRIPT_DIR} pytest --keep -svv ${SCRIPT_DIR}/e2e.py
else
     PYTHONPATH=${SCRIPT_DIR} pytest -svv ${SCRIPT_DIR}/e2e.py
fi

deactivate
