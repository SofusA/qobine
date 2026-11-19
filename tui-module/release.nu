let date = (date now | format date "%Y-%m-%d")
let cargo_version = ($date | str replace -a "-" ".")
let tag = $"v($date)"

let unreleased = "unreleased.md"
let release_notes = $"($date).md"

# Ensure unreleased.md exists
if not ($unreleased | path exists) {
    error make {
        msg: $"Missing ($unreleased)"
    }
}

# Ensure working copy is clean
let diff = (jj diff --summary | str trim)

if ($diff | is-not-empty) {
    error make {
        msg: "Working copy contains changes."
    }
}

# Rename release notes
mv $unreleased $release_notes

# Create next release notes file
"" | save $unreleased

# Update Cargo version
open Cargo.toml
| update package.version $cargo_version
| save --force Cargo.toml

# Commit release metadata
jj commit -m $"Release ($tag)"

# Push branch/bookmarks
jj git push

# Tag the commit currently referenced by main
let main_commit = jj log -r main --no-graph -T "commit_id" | str trim

git tag $tag $main_commit
git push origin $tag

# Create GitHub release
gh release create $tag --title $tag --notes-file $release_notes

print $"Created release ($tag)"
