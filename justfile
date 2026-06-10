init-database:
    cargo sqlx db create && cargo sqlx migrate run --source qobuz-player-player/migrations

[working-directory: 'qobuz-player-web']
build-styles:
    npm i
    npm run build

[working-directory: 'qobuz-player-web']
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
