"""Pakkaa Chillyn Windows-binäärin Microsoft Store -yhteensopivaksi MSIX-paketiksi.

Käyttö (kun `cargo build --release` tai `npx tauri build` on ajettu):
    python -X utf8 tools/pakkaa-msix.py
    python -X utf8 tools/pakkaa-msix.py --revision 1

Partner Centerin tunnistetiedot luetaan järjestyksessä:
  1. msix/store.local.json (jos olemassa, ei gitissä)
  2. msix/config.json
  3. CLI-vipujen ylikirjoitukset (--identity-name, --publisher, --publisher-display-name, --display-name)

Microsoft Store allekirjoittaa ladatun .msix-tiedoston omalla varmenteellaan,
joten Storeen lähetettävää pakettia ei tarvitse allekirjoittaa itse.
"""
import argparse
import glob
import json
import os
import shutil
import subprocess
import sys
from xml.sax.saxutils import escape as xml_esc

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MSIX_DIR = os.path.join(ROOT, "msix")
STAGING = os.path.join(MSIX_DIR, "staging")
DIST = os.path.join(ROOT, "dist")
EXE = os.path.join(ROOT, "src-tauri", "target", "release", "chilly.exe")
ICON_SRC = os.path.join(ROOT, "src-tauri", "icons", "icon.png")


def find_makeappx() -> str:
    hits = sorted(glob.glob(r"C:\Program Files (x86)\Windows Kits\10\bin\*\x64\makeappx.exe"))
    if not hits:
        sys.exit("makeappx.exe ei löytynyt (asenna Windows 10/11 SDK).")
    return hits[-1]


def make_assets(assets_dir: str) -> None:
    os.makedirs(assets_dir, exist_ok=True)
    try:
        from PIL import Image
    except ImportError:
        sys.exit("Pillow (PIL) puuttuu. Asenna: python -m pip install pillow")

    src = Image.open(ICON_SRC).convert("RGBA")

    def save_square(name: str, size: int, pad: float = 0.0) -> None:
        canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
        inner = max(1, round(size * (1.0 - 2.0 * pad)))
        im = src.resize((inner, inner), Image.Resampling.LANCZOS)
        off = (size - inner) // 2
        canvas.paste(im, (off, off), im)
        canvas.save(os.path.join(assets_dir, name), "PNG")

    def save_wide(name: str, w: int, h: int, icon_frac: float = 0.78) -> None:
        canvas = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        s = max(1, round(min(w, h) * icon_frac))
        im = src.resize((s, s), Image.Resampling.LANCZOS)
        canvas.paste(im, ((w - s) // 2, (h - s) // 2), im)
        canvas.save(os.path.join(assets_dir, name), "PNG")

    # Pakolliset + suositellut Microsoft Store / Windows 11 MSIX -kuvakkeet
    save_square("StoreLogo.png", 50)
    save_square("Square44x44Logo.png", 44)
    save_square("Square44x44Logo.targetsize-44_altform-unplated.png", 44)
    save_square("Square44x44Logo.targetsize-256_altform-unplated.png", 256)
    save_square("Square150x150Logo.png", 150, pad=0.08)
    save_square("Square310x310Logo.png", 310, pad=0.08)
    save_wide("Wide310x150Logo.png", 310, 150, icon_frac=0.80)
    save_wide("SplashScreen.png", 620, 300, icon_frac=0.72)


def build_manifest(cfg: dict, msix_ver: str) -> str:
    return f"""<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap10/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  xmlns:desktop="http://schemas.microsoft.com/appx/manifest/desktop/windows10"
  IgnorableNamespaces="uap uap10 rescap desktop">

  <Identity
    Name="{xml_esc(cfg['identity_name'])}"
    Publisher="{xml_esc(cfg['publisher'])}"
    Version="{xml_esc(msix_ver)}"
    ProcessorArchitecture="x64" />

  <Properties>
    <DisplayName>{xml_esc(cfg['display_name'])}</DisplayName>
    <PublisherDisplayName>{xml_esc(cfg['publisher_display_name'])}</PublisherDisplayName>
    <Logo>Assets\\StoreLogo.png</Logo>
  </Properties>

  <Resources>
    <Resource Language="fi-FI" />
    <Resource Language="en-US" />
  </Resources>

  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>

  <Capabilities>
    <Capability Name="internetClient" />
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>

  <Applications>
    <Application
      Id="Chilly"
      Executable="chilly.exe"
      EntryPoint="Windows.FullTrustApplication"
      uap10:RuntimeBehavior="packagedClassicApp"
      uap10:TrustLevel="mediumIL">
      <uap:VisualElements
        DisplayName="{xml_esc(cfg['display_name'])}"
        Description="{xml_esc(cfg['description'])}"
        BackgroundColor="transparent"
        Square150x150Logo="Assets\\Square150x150Logo.png"
        Square44x44Logo="Assets\\Square44x44Logo.png">
        <uap:DefaultTile
          Wide310x150Logo="Assets\\Wide310x150Logo.png"
          Square310x310Logo="Assets\\Square310x310Logo.png" />
        <uap:SplashScreen Image="Assets\\SplashScreen.png" BackgroundColor="#0d0f14" />
      </uap:VisualElements>
      <Extensions>
        <desktop:Extension Category="windows.startupTask" Executable="chilly.exe" EntryPoint="Windows.FullTrustApplication">
          <desktop:StartupTask TaskId="ChillyStartup" Enabled="false" DisplayName="{xml_esc(cfg['display_name'])}" />
        </desktop:Extension>
      </Extensions>
    </Application>
  </Applications>
</Package>
"""


def main() -> None:
    ap = argparse.ArgumentParser(description="Pakkaa Chilly MSIX-paketiksi Microsoft Storea varten.")
    ap.add_argument("--revision", type=int, default=0, help="4. versionumero (oletus 0 -> x.y.z.0)")
    ap.add_argument("--identity-name", help="Partner Center: Package/Identity/Name")
    ap.add_argument("--publisher", help="Partner Center: Package/Identity/Publisher (CN=...)")
    ap.add_argument("--publisher-display-name", help="Partner Center: PublisherDisplayName")
    ap.add_argument("--display-name", help="Partner Center: varattu sovelluksen nimi")
    args = ap.parse_args()

    if not os.path.exists(EXE):
        sys.exit(f"Puuttuu {EXE} – aja ensin cargo build --release tai npx tauri build.")

    ver = json.load(open(os.path.join(ROOT, "src-tauri", "tauri.conf.json"), encoding="utf-8"))["version"]
    msix_ver = f"{ver}.{args.revision}"

    cfg = json.load(open(os.path.join(MSIX_DIR, "config.json"), encoding="utf-8"))
    local_cfg = os.path.join(MSIX_DIR, "store.local.json")
    if os.path.exists(local_cfg):
        cfg.update(json.load(open(local_cfg, encoding="utf-8")))
    if args.identity_name:
        cfg["identity_name"] = args.identity_name
    if args.publisher:
        cfg["publisher"] = args.publisher
    if args.publisher_display_name:
        cfg["publisher_display_name"] = args.publisher_display_name
    if args.display_name:
        cfg["display_name"] = args.display_name

    if os.path.exists(STAGING):
        shutil.rmtree(STAGING)
    os.makedirs(STAGING, exist_ok=True)
    os.makedirs(DIST, exist_ok=True)

    shutil.copy2(EXE, os.path.join(STAGING, "chilly.exe"))
    make_assets(os.path.join(STAGING, "Assets"))
    with open(os.path.join(STAGING, "AppxManifest.xml"), "w", encoding="utf-8", newline="\n") as f:
        f.write(build_manifest(cfg, msix_ver))

    out = os.path.join(DIST, f"Chilly_{ver}_x64.msix")
    makeappx = find_makeappx()
    r = subprocess.run([makeappx, "pack", "/d", STAGING, "/p", out, "/o"], capture_output=True, text=True)
    if r.returncode != 0:
        print(r.stdout)
        print(r.stderr, file=sys.stderr)
        sys.exit(f"makeappx epäonnistui (rc={r.returncode})")

    size_mb = os.path.getsize(out) / (1024 * 1024)
    print(f"Valmis: {out} ({size_mb:.2f} Mt, versio {msix_ver})")
    if "PLACEHOLDER" in cfg["publisher"]:
        print("HUOM: Publisher on vielä PLACEHOLDER – täytä Partner Centerin tunnistetiedot ennen Store-latausta!")


if __name__ == "__main__":
    main()
