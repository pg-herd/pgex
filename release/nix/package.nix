{ lib, stdenv, stdenvNoCC, fetchurl, libpq, autoPatchelfHook, cctools, darwin }:

let
  release = builtins.fromJSON (builtins.readFile ./sources.json);
  platform = release.platforms.${stdenv.hostPlatform.system};
in stdenvNoCC.mkDerivation {
  pname = "pgex";
  inherit (release) version;
  src = fetchurl {
    url = "https://github.com/pg-herd/pgex/releases/download/v${release.version}/pgex-v${release.version}-${platform.target}.tar.gz";
    sha256 = platform.sha256;
  };

  nativeBuildInputs = lib.optionals stdenv.hostPlatform.isLinux [ autoPatchelfHook ]
    ++ lib.optionals stdenv.hostPlatform.isDarwin [ cctools darwin.autoSignDarwinBinariesHook ];
  buildInputs = [ libpq ] ++ lib.optionals stdenv.hostPlatform.isLinux [ stdenv.cc.cc.lib ];
  dontBuild = true;
  dontStrip = true;

  installPhase = ''
    runHook preInstall
    install -Dm755 pgex "$out/bin/pgex"
    install -Dm644 LICENSE "$out/share/doc/pgex/LICENSE"
    cp -R licenses "$out/share/doc/pgex/licenses"
    install -m644 README.md "$out/share/doc/pgex/README.md"
    ${lib.optionalString stdenv.hostPlatform.isDarwin ''
      install_name_tool -change /opt/homebrew/opt/libpq/lib/libpq.5.dylib \
        ${lib.getLib libpq}/lib/libpq.5.dylib "$out/bin/pgex"
    ''}
    runHook postInstall
  '';

  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck
    LD_BIND_NOW=1 "$out/bin/pgex" --help > help.txt
    grep -F 'Usage: pgex [OPTIONS]' help.txt
    status=0
    "$out/bin/pgex" --source postgresql://localhost/source \
      --target postgresql://localhost/target --where 'id =' > diagnostic.txt 2>&1 || status=$?
    test "$status" = 2
    grep -F 'Invalid --where predicate' diagnostic.txt
    grep -F 'Line 1, column 5' diagnostic.txt
    test -f "$out/share/doc/pgex/licenses/libpg_query.txt"
    runHook postInstallCheck
  '';

  meta = {
    description = "Export PostgreSQL data";
    homepage = "https://github.com/pg-herd/pgex";
    license = lib.licenses.mit;
    mainProgram = "pgex";
    platforms = builtins.attrNames release.platforms;
    sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
  };
}
