import json
import logging
import subprocess

log = logging.getLogger(__name__)


class Terraform:
    def __init__(self, workdir=None):
        self.workdir = workdir

    def init(self):
        out, err = self._cmd("init")

        return out, err

    def apply(self):
        out, err = self._cmd("apply", "-auto-approve", "-no-color")

        return out, err

    def show(self):
        data, err = self._cmd("show", "-json", "-no-color")

        return json.loads(data), err

    def destroy(self):
        out, err = self._cmd("destroy", "-auto-approve", "-no-color")

        return out, err

    def _cmd(self, executeable, *args):
        cmd = ["terraform"]
        if self.workdir:
            cmd.append(f"-chdir={self.workdir}")

        cmd.append(executeable)
        cmd.extend(args)

        proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        stdout, stderr = proc.communicate()

        return stdout.decode("utf-8"), stderr.decode("utf-8")
