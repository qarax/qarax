import functools
import logging
import random
import threading
import time

import paramiko

log = logging.getLogger(__name__)


def download_file_to_host(url, host, username, password, dest_path):
    ssh = paramiko.SSHClient()
    ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())

    retry(
        functools.partial(
            ssh.connect, host, username=username, password=password
        ),
        attempts=4, delay=20, backoff=1
    )

    log.info("Downloading from %s to %s", url, dest_path)
    ssh.exec_command(f'wget -O {dest_path} {url}', get_pty=True)


def wait_for_status(func, timeout=60, status='up', step=1):
    with Timer(timeout=timeout) as timer:
        while not timer.passed():
            if func() == status:
                return True
            time.sleep(step)

    return False


def retry(func, attempts=3, delay=1, backoff=2, jitter=0.1):
    while attempts > 0:
        try:
            return func()
        except Exception as e:
            log.warning("%s, retrying in %d seconds", str(e), delay)
            attempts -= 1
            if attempts > 0:
                time.sleep(delay + (random.uniform(0, jitter)))
                delay *= backoff


def run_parallel_jobs(jobs):
    threads = []
    for job in jobs:
        thread = threading.Thread(target=job)
        thread.start()
        threads.append(thread)

    for thread in threads:
        thread.join()


class Timer:
    def __init__(self, timeout=60):
        self.timeout = timeout

    def __enter__(self):
        self.start = time.time()
        return self

    def __exit__(self, *args):
        pass

    def passed(self):
        return (time.time() - self.start) > self.timeout
