; Chillyn asennusohjelman lisäykset.
; Asennuksen alussa valittu kieli välitetään Chillylle rekisterin kautta.
; Chilly lukee arvon ensimmäisellä käynnistyksellä, ottaa sen käyttöön ja poistaa sen.
; Automaattisessa päivityksessä (/UPDATE) kieltä ei kysytä, joten käyttäjän kieli jätetään ennalleen.
!macro NSIS_HOOK_POSTINSTALL
  ${If} $UpdateMode <> 1
    ${If} $LANGUAGE == 1035
      WriteRegStr HKCU "Software\Chilly" "Language" "fi"
    ${Else}
      WriteRegStr HKCU "Software\Chilly" "Language" "en"
    ${EndIf}
  ${EndIf}
!macroend
