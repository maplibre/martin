# Development

## docker

Install [docker](https://docs.docker.com/get-docker/) and [docker-compose](https://docs.docker.com/compose/)

## just

Install [Just](https://github.com/casey/just#readme):

```bash, ignore
cargo install just --locked
just validate-tools  # verify setup
```

## Other Required tools

We provide a simple command to check that all requirements are setup

```bash, ignore
just validate-tools
```

## Git setup

The recommended setup for both core and casual contributors is to **always** create a fork
of the primary repo under their own account. The local repo should have two remotes: `upstream` pointing to the primary `maplibre/martin` repo, and `origin` pointing to the user's own fork. The `main` branch should track `upstream/main`, but all new work will be pushed to `origin` and PRs will be created from there.

<details><summary>Rationale for this setup (click to expand)</summary>

<small>This rationale was copied from [a post](https://gist.github.com/nyurik/4e299ad832fd2dd43d2b27191ed3ec30) by Yuri</small>

Open source contribution is both a technical and a social phenomenon.
Any FOSS project naturally has a "caste system" - a group
of contributors with extensive rights vs everyone else. Some of this separation
is necessary - core contributors have deeper knowledge of the code, share vision,
and trust each other.

Core contributors have one more right that others do not -- they can create repository branches.
Thus, they can contribute "locally" - by pushing proposed changes to the primary repository's work branches,
and create "local" pull requests inside the same repo.  This is different from others,
who can contribute only from their own forks.

There is little difference between creating pull requests from one's own fork and from the primary repo,
and there are a few reasons why core contributors should **never** do it from the primary repo:

* it ensures that casual contributors always run the same CI as core contributors. If contribution process breaks, it will affect everyone, and will get fixed faster.
* it puts everyone on the same leveled playing field, reducing the "caste system" effect, making the project feel more welcoming to new contributors
* it ensures that the primary repo only has maintained branches (e.g. `main` and `v1.x`),
  not a bunch of PR branches whose ownership and work status is unclear to everyone

In the martin repository, we follow this and have a branch protection rule that prevents core contributors from creating pull requests from the primary repo.

</details>

```bash
# clone the primary fork to your local machine, naming the remote "upstream"
# make sure to replace the URL with the correct one
git clone -o upstream https://github.com/maplibre/martin.git
cd martin

# add your own fork as a remote, naming it "origin"
git remote add origin https://github.com/nyurik/martin.git
```

For further setup instructions for IDEs, please see the [Getting Involved](getting-involved.md) step after you have installed the necessary tools below.
<details><summary>If you have already cloned the repo locally, use this guide to update your setup (click to expand)</summary>

If you already cloned the repo locally, you can update it to use the new setup. This assumes you have a local clone of the repo, the remote name is `origin`, and you have already forked the repo on GitHub.

```bash
# Getting a quick glance about your remotes: git remote -v
git remote -v
# Rename the existing remote to "upstream". Your "main" branch will now track "upstream/main"
git remote rename origin upstream

# Add your own fork as a remote, naming it "origin" (adjust the URL)
git remote add origin https://github.com/nyurik/martin.git
```

</details>

## Contributing New Code

```bash
# switch to main branch (tracking upstream/main), and pull the latest changes
git switch main
git fetch upstream

# create a new branch for your work
git switch -c my-new-feature

# edit files, and commit changes
# '-a' will add all modified files
# `-m` allows you to add a short commit message
git commit -a -m "My new feature"

# push the changes to your own fork
# '-u' will track your local branch with the remote
git push -u origin my-new-feature

# Click the link shown by `git push` in the terminal to create a pull request
# from your fork using the GitHub web interface
```

When developing MBTiles SQL code, you may need to use `just prepare-sqlite` whenever SQL queries are modified.

## Quick Start

```bash
# Install tools
cargo install just --locked
just validate-tools

# Start development
just start  # test database
just run    # Martin server
just test   # verify setup
```

## Working with Just

```bash
just help        # common commands
just --list      # all commands
just validate-tools  # check setup
```

### Development workflow

```bash
just start       # start test database
just run         # start Martin server
just test        # run all tests
just fmt         # format code
just clippy      # lint code
just book        # build documentation
just stop        # stop test database
```

### Pass arguments to commands

```bash
just test-cargo -- --test integration_test
just run --config /path/to/config.yaml
```


