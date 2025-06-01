{ config, lib, pkgs, ... }:

let
  cfg = config.services.screendbtui;
  meta = {
    options = {
      enable = lib.options.mkEnableOption "Enable the Rust application service";
      settings = lib.options.mkOption {
        type = lib.types.submodule {
          options = {
            # port = lib.options.mkOption {
            #   type = lib.types.int;
            #   default = 8080;
            #   description = "Port number to listen on";
            # };
            
            # debug-mode = lib.options.mkOption {
            #   type = lib.types.bool;
            #   default = false;
            #   description = "Enable debug mode";
            # };
          };
        };
        default = {};
        description = "Application settings";
      };
    };
  };

in {
  options.services.screendbtui = meta.options;

  config = lib.mkIf cfg.enable {
    systemd.services.screendbtui = {
      description = "Make Screen DB TUI available";
      after = [ "network.target" ];
      
      serviceConfig = {
        Type = "simple";
        ExecStart = "${pkgs.rust}/bin/cargo run --release";
        Environment = lib.optionalString cfg.settings.debug-mode "RUST_LOG=debug";
        
        DynamicUser = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
      };
    };
  };
}