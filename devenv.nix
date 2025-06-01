{
  pkgs,
  inputs,
  ...
}: {
  languages.nix.enable = true;
  languages.rust.enable = true;
  difftastic.enable = true;

  packages = [
    # Add formatting tools to the environment
    pkgs.taplo # TOML formatter
    pkgs.yamllint # YAML linter
    pkgs.yamlfmt # YAML formatter
    pkgs.markdownlint # Markdown linter
    pkgs.mdformat # Markdown formatter

    # # libs
    # pkg-config
    # libressl # openssl
    # # sqlite
  ];

  # Add shell hooks for automatic formatting
  enterShell = ''
    # TOML formatting
    alias fmt-toml="taplo fmt -i"

    # YAML formatting and linting
    alias fmt-yaml="yamlfmt -i"
    alias lint-yaml="yamllint -f auto"

    # Markdown formatting and linting
    alias fmt-md="mdformat -i"
    alias lint-md="markdownlint -f ."
  '';

  git-hooks.hooks = {
    # lint shell scripts
    shellcheck.enable = true;
    # execute example shell from Markdown files
    mdsh.enable = true;

    taplo.enable = true; # TOML formatting
    yamllint.enable = true; # YAML linting
    yamlfmt.enable = true; # YAML formatting
    markdownlint.enable = true; # Markdown linting

    # some hooks have more than one package, like clippy:
    clippy.enable = true;
    clippy.packageOverrides.cargo = pkgs.cargo;
    clippy.packageOverrides.clippy = pkgs.clippy;
    # some hooks provide settings
    clippy.settings.allFeatures = true;

    unit-tests = {
      enable = true;

      # The name of the hook (appears on the report table):
      name = "Unit tests";

      # The command to execute (mandatory):
      entry = "cargo test";

      # The pattern of files to run on (default: "" (all))
      # see also https://pre-commit.com/#hooks-files
      #   files = "\\.(c|h)$";

      # List of file types to run on (default: [ "file" ] (all files))
      # see also https://pre-commit.com/#filtering-files-with-types
      # You probably only need to specify one of `files` or `types`:
      types = ["text" "rs"];

      # Exclude files that were matched by these patterns (default: [ ] (none)):
      #   excludes = ["irrelevant\\.c"];

      # The language of the hook - tells pre-commit
      # how to install the hook (default: "system")
      # see also https://pre-commit.com/#supported-languages
      language = "system";

      # Set this to false to not pass the changed files
      # to the command (default: true):
      pass_filenames = false;
    };
  };
}
