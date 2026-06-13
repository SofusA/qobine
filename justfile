init-database:
    cargo sqlx db create && cargo sqlx migrate run --source player-module/migrations

[working-directory: 'web-module']
build-styles:
    npm i
    npm run build

[working-directory: 'web-module']
build-assets:
    npm i
    npm run build-assets

create-env-file:
    echo 'DATABASE_URL="sqlite:///tmp/qobuz-player.db"' > .env

build-all:
    just create-env-file
    just init-database
    just build-styles
    just build-assets
    cargo build --release
