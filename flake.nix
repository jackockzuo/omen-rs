{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "https://channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz";
    utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      utils,
      naersk,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };
      in
      {
        packages.default = naersk-lib.buildPackage ./.;

        apps.omen = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/omen";
        };

        devShells.default =
          with pkgs;
          mkShell {
            buildInputs = [
              cargo
              rustc
              rustfmt
              clippy
            ];
            RUST_SRC_PATH = rustPlatform.rustLibSrc;
          };
      }
    )
    // {
      nixosModules.default =
        { config, lib, pkgs, ... }:
        let
          cfg = config.services.omen;
          # pkgs.system 在 nixpkgs 2025-10-28 起触发弃用告警
          # （"'system' has been renamed to/replaced by 'stdenv.hostPlatform.system'"），
          # 必须用 stdenv.hostPlatform.system；注意在跨编译主机上两者等价（hostPlatform = 主机）。
          omen-pkg = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        in
        {
          options.services.omen = {
            enable = lib.mkEnableOption "OMEN 性能控制（EC 解锁 + 守护进程）";

            performance = lib.mkOption {
              type = lib.types.enum [ "performance" "balanced" ];
              default = "performance";
              description = "性能模式：performance 解锁 130W，balanced 恢复 55W";
            };

            holdInterval = lib.mkOption {
              type = lib.types.ints.positive;
              default = 30;
              description = "omend hold 看门狗刷新间隔（秒）";
            };

            batteryCare = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = "开启电池养护（充电限制 80%）";
            };

            logLevel = lib.mkOption {
              type = lib.types.str;
              default = "info";
              description = "tracing 日志级别（error/warn/info/debug/trace）";
            };

            fanCurve = lib.mkOption {
              type = lib.types.listOf (lib.types.submodule {
                options = {
                  temp = lib.mkOption {
                    type = lib.types.ints.between 0 120;
                    description = "温度阈值（°C）";
                  };
                  speed = lib.mkOption {
                    type = lib.types.ints.between 0 100;
                    description = "风扇转速（%）";
                  };
                };
              });
              default = [ ];
              description = ''
                温度曲线点列表（按温度升序），omend 根据 CPU 温度（hwmon coretemp/k10temp）
                线性插值调风扇。
                为空则不自动调风扇，由 BIOS 控制。
                示例: [ { temp=50; speed=30; } { temp=65; speed=50; } { temp=80; speed=80; } { temp=90; speed=100; } ]
              '';
            };
          };

          config = lib.mkIf cfg.enable {
            boot = {
              extraModulePackages = [ config.boot.kernelPackages.acpi_call ];
              extraModprobeConfig = "options ec_sys write_support=1";
              kernelModules = [ "hp-wmi" "acpi_call" "ec_sys" ];
            };

            environment.systemPackages = [ omen-pkg ];

            systemd.services.omen-unlock = {
              description = "Apply OMEN EC settings on boot";
              wantedBy = [ "multi-user.target" ];
              serviceConfig = {
                Type = "oneshot";
                RemainAfterExit = true;
              };
              script =
                let
                  perf-cmd = if cfg.performance == "performance" then "unlock" else "balanced";
                  battery-cmd = lib.optionalString cfg.batteryCare ''
                    ${omen-pkg}/bin/omen battery on
                  '';
                in
                ''
                  ${omen-pkg}/bin/omen ${perf-cmd}
                  ${battery-cmd}
                '';
            };

            systemd.services.omend = {
              description = "OMEN EC hold watchdog";
              after = [ "omen-unlock.service" ];
              wantedBy = [ "multi-user.target" ];
              environment = {
                OMEN_PERF = cfg.performance;
                OMEN_HOLD_INTERVAL = toString cfg.holdInterval;
                OMEN_BATTERY_CARE = if cfg.batteryCare then "1" else "0";
                OMEN_FAN_CURVE = lib.concatStringsSep "," (
                  map (p: "${toString p.temp}:${toString p.speed}") cfg.fanCurve
                );
                RUST_LOG = cfg.logLevel;
              };
              serviceConfig = {
                Type = "simple";
                ExecStart = "${omen-pkg}/bin/omend";
                Restart = "on-failure";
                RestartSec = 5;
                ReadWritePaths = [
                  "/sys/kernel/debug/ec"
                  "/sys/firmware/acpi"
                  "/proc/acpi"
                  "/tmp"
                ];
              };
            };
          };
        };
    };
}
