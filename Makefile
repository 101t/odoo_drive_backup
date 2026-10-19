.PHONY: help build frontend backend test lint dev-backend dev-frontend docker up down clean

BIN := target/release/odoo-backup

help:
	@echo "make build         Build the web UI and the release binary (UI embedded)"
	@echo "make test          Run backend tests and the frontend type-check"
	@echo "make lint          cargo clippy + cargo fmt --check"
	@echo "make dev-backend   Run the API on :8080 with ./data as data directory"
	@echo "make dev-frontend  Run Vite on :5173 (proxies /api to :8080)"
	@echo "make docker        Build the odoo-backup:latest image"
	@echo "make up / down     docker compose up -d / down"

build: frontend backend

frontend:
	cd frontend && npm ci --no-audit --no-fund && npm run build

backend:
	cargo build --release --locked -p odoo-backup
	@echo "Binary: $(BIN)"

test:
	cargo test -p odoo-backup
	cd frontend && npm run typecheck

lint:
	cargo clippy --all-targets -- -D warnings
	cargo fmt --check

dev-backend:
	cargo run -p odoo-backup -- --data-dir ./data serve

dev-frontend:
	cd frontend && npm run dev

docker:
	docker build -t odoo-backup:latest .

up:
	docker compose up -d --build

down:
	docker compose down

clean:
	cargo clean
	rm -rf frontend/dist frontend/node_modules
