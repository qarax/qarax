import pytest

def pytest_addoption(parser):
    parser.addoption("--keep", action="store", default=False)
