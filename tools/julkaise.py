"""Chillyn julkaisu sivulle (samppalandia.wtf/chilly).

Käyttö (kun `npx tauri build` on ajettu allekirjoitusavaimen kanssa):
    python tools/julkaise.py "Muutokset suomeksi" [lisätiedosto sivulla ...]

Tekee:
  1. kopioi allekirjoitetun asennusohjelman kansioihin dist/ ja site/ (vanhat versiot pois)
  2. kirjoittaa site/latest.json (Chillyn päivitystarkistus lukee tämän)
  3. päivittää sivun versionumerot ja latauslinkit (muutoslokin rivit pysyvät ennallaan)
  4. tekee zipin dist/chilly-sivu-<versio>-paivitys.zip → puretaan public_html-kansioon

Päivitysilmoitus lähtee käyttäjille vasta, kun zip on purettu palvelimelle.
"""
import datetime
import glob
import json
import os
import re
import shutil
import sys
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SITE = os.path.join(ROOT, "site")
DIST = os.path.join(ROOT, "dist")
BUNDLE = os.path.join(ROOT, "src-tauri", "target", "release", "bundle", "nsis")
BASE_URL = "https://samppalandia.wtf/chilly/"


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    notes = sys.argv[1]
    extra = sys.argv[2:]
    ver = json.load(open(os.path.join(ROOT, "src-tauri", "tauri.conf.json"), encoding="utf-8"))["version"]
    exe_name = f"Chilly_{ver}_x64-setup.exe"
    exe = os.path.join(BUNDLE, exe_name)
    sig = exe + ".sig"
    if not (os.path.exists(exe) and os.path.exists(sig)):
        sys.exit(f"Puuttuu {exe} tai .sig – aja ensin npx tauri build (TAURI_SIGNING_PRIVATE_KEY asetettuna).")

    # 1. asennusohjelma dist/ ja site/
    for folder in (DIST, SITE):
        for old in glob.glob(os.path.join(folder, "Chilly_*_x64-setup.exe")):
            if os.path.basename(old) != exe_name:
                os.remove(old)
        shutil.copy2(exe, os.path.join(folder, exe_name))

    # 2. latest.json
    latest = {
        "version": ver,
        "notes": notes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {"windows-x86_64": {"signature": open(sig, encoding="utf-8").read().strip(), "url": BASE_URL + exe_name}},
    }
    with open(os.path.join(SITE, "latest.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump(latest, f, ensure_ascii=False, indent=2)

    # 3. sivun versionumerot (ei muutoslokin <div class="v">-rivejä)
    p = os.path.join(SITE, "index.html")
    s = open(p, encoding="utf-8").read()
    s = re.sub(r"Chilly_\d+\.\d+\.\d+_x64-setup\.exe", exe_name, s)
    s = re.sub(r"BETA \d+\.\d+\.\d+", f"BETA {ver}", s)
    s = re.sub(r">Beta \d+\.\d+\.\d+<", f">Beta {ver}<", s)
    s = re.sub(r"<b>Chilly \d+\.\d+\.\d+ <span", f"<b>Chilly {ver} <span", s)
    open(p, "w", encoding="utf-8", newline="\n").write(s)

    lue = os.path.join(DIST, "LUE-MINUT.txt")
    if os.path.exists(lue):
        t = open(lue, encoding="utf-8").read()
        open(lue, "w", encoding="utf-8").write(re.sub(r"Chilly_\d+\.\d+\.\d+_x64", f"Chilly_{ver}_x64", t))

    # 4. päivityszip
    files = ["index.html", "latest.json", exe_name, "assets/site.css", "assets/site.js", "assets/icons.js"]
    for opt in ("assets/header.css", "assets/header.js", ".htaccess"):
        if os.path.exists(os.path.join(SITE, opt)) and opt not in extra:
            files.append(opt)
    files += extra
    out = os.path.join(DIST, f"chilly-sivu-{ver}-paivitys.zip")
    for old in glob.glob(os.path.join(DIST, "chilly-sivu-*-paivitys.zip")):
        os.remove(old)
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        for f in files:
            z.write(os.path.join(SITE, f), "chilly/" + f)
    print(f"Valmis: {exe_name}, latest.json ({ver}), {os.path.basename(out)} ({os.path.getsize(out) // 1024} kt)")


if __name__ == "__main__":
    main()
