import logging
import logging.config
import os

log_file_path = os.path.join(
    os.path.dirname(
        os.path.abspath(__file__)),
    'logger.conf')

logging.config.fileConfig(log_file_path)
