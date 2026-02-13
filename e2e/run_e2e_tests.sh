#!/usr/bin/env bash

set -e

echo "===== Qarax E2E Test Runner ====="

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Navigate to E2E directory
cd "$(dirname "$0")"

# Environment variables:
#   REBUILD=1       - Force rebuild of all images
#   KEEP=1          - Keep services running after tests (for debugging)
#   SKIP_BUILD=1    - Skip building qarax-node binary

cleanup() {
    if [ -n "$KEEP" ]; then
        echo ""
        echo -e "${GREEN}Services kept running (KEEP=1)${NC}"
        echo ""
        echo "Useful commands:"
        echo "  docker-compose logs -f           # Follow logs"
        echo "  docker-compose logs qarax-node   # qarax-node logs"
        echo "  docker-compose exec qarax-node sh  # Shell into qarax-node"
        echo "  docker-compose down -v           # Stop and remove everything"
        echo ""
        echo "Test endpoint:"
        echo "  curl http://localhost:8000/vms"
        echo ""
    else
        echo -e "${YELLOW}Cleaning up...${NC}"
        docker-compose down -v
    fi
}

trap cleanup EXIT

# Build qarax-node binary if needed
if [ -z "$SKIP_BUILD" ]; then
    if [ -n "$REBUILD" ] || [ ! -f ../target/x86_64-unknown-linux-musl/release/qarax-node ]; then
        echo -e "${YELLOW}Building qarax-node binary...${NC}"
        cd ..
        cargo build --release -p qarax-node
        cd e2e
    else
        echo -e "${GREEN}Using existing qarax-node binary${NC}"
        echo -e "${YELLOW}To rebuild, run: REBUILD=1 ./run_e2e_tests.sh${NC}"
    fi
fi

# Build and start services
echo -e "${YELLOW}Starting services...${NC}"
if [ -n "$REBUILD" ]; then
    docker-compose build --no-cache
fi
docker-compose up -d --build

# Wait for services to be healthy
echo -e "${YELLOW}Waiting for services to be healthy...${NC}"
timeout=90
elapsed=0
while [ $elapsed -lt $timeout ]; do
    # Check if all services are healthy
    healthy_count=$(docker-compose ps 2>/dev/null | grep -c "(healthy)" || echo "0")
    total_services=3  # postgres, qarax, qarax-node
    
    if [ "$healthy_count" -ge "$total_services" ]; then
        echo ""
        echo -e "${GREEN}All services are healthy!${NC}"
        break
    fi
    
    # Check for failed services
    if docker-compose ps 2>/dev/null | grep -q "Exit"; then
        echo ""
        echo -e "${RED}A service has failed!${NC}"
        docker-compose ps
        docker-compose logs
        exit 1
    fi
    
    echo -n "."
    sleep 2
    elapsed=$((elapsed + 2))
done

if [ $elapsed -ge $timeout ]; then
    echo ""
    echo -e "${RED}Timeout waiting for services to be healthy${NC}"
    docker-compose ps
    docker-compose logs
    exit 1
fi

# Show service status
echo ""
echo -e "${YELLOW}Service status:${NC}"
docker-compose ps
echo ""

# Setup Python environment with uv
echo -e "${YELLOW}Installing test dependencies...${NC}"
uv sync --frozen

# Run the tests
echo -e "${YELLOW}Running E2E tests...${NC}"
if uv run pytest test_vm_lifecycle.py -v "$@"; then
    echo ""
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo ""
    echo -e "${RED}Tests failed!${NC}"
    echo ""
    echo -e "${YELLOW}Service logs:${NC}"
    docker-compose logs --tail=50
    echo ""
    
    if [ -z "$KEEP" ]; then
        echo -e "${YELLOW}To keep services running for debugging:${NC}"
        echo -e "${GREEN}  KEEP=1 ./run_e2e_tests.sh${NC}"
        echo ""
    fi
    
    exit 1
fi
