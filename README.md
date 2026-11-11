# Dima 3D

<img src="assets/icona_256.png" alt="Icona di Dima 3D" width="96" align="right">

Applicazione desktop per Windows, scritta in Rust, per aprire, ispezionare, posizionare ed esportare
matematiche 3D di grandi dimensioni (centinaia di MB), come le scocche e i componenti automobilistici.
Il nome viene dalla *dima* di officina, la maschera che tiene i pezzi nella posizione giusta.

> **Codice scritto interamente con l'intelligenza artificiale.** Tutto il codice di questo progetto
> (programma, convertitori, test, script e documentazione) è stato scritto da Claude (Anthropic) con
> Claude Code. L'autore del progetto non ha scritto codice: ha definito le richieste, provato il
> programma sulle matematiche reali e deciso cosa correggere.

## Funzioni

- **Apertura** di WRL (VRML 2.0 e 1.0), STEP, JT 8/9/10, STL, HSF e Open Inventor (`.iv`, testo e binario),
  anche più file nella stessa scena, con conversione delle unità (millimetri ↔ metri)
- **STEP**: visualizzazione rapida oppure conversione in mesh con deviazione superficiale, angolare e relativa
  regolabili (convertitore `tools\step2wrl`, basato su OpenCASCADE)
- **JT**: assiemi in più file, con il convertitore `tools\jt2wrl.exe` (basato su JTReader)
- **Vista 3D** con wgpu: antialiasing, illuminazione che segue la vista, spigoli vivi, sfondo uniforme o sfumato;
  navigazione in stile OpenInventor, viste standard in coordinate veicolo, cubo di orientamento cliccabile
- **Albero della scena** con ricerca, raggruppamento automatico dei pezzi connessi, colori, rinomina,
  nascondi/isola/elimina, Annulla/Ripeti
- **Misure** tra due punti con aggancio ai vertici; con la *calamita* i punti seguono i pezzi che si spostano
- **Piano di sezione**, proprietà del pezzo selezionato, pipetta per i colori
- **Posizione**: sposta e ruota file o pezzi in X Y Z / A B C, in coordinate dell'oggetto o della cabina;
  calcolo e applicazione di offset tra oggetti o tra `CONST BASE` (es. per costruire lo skid)
- **Cerniere**: rotazione attorno a un asse scelto nella vista (due punti, due fori, un foro, punto + asse),
  oppure calcolato da due matematiche dello stesso pezzo in posizioni diverse; porta una misura a una quota data
- **Cerniere a 4 perni** (cofani): dai quattro perni il cofano si apre dell'angolo scelto e le bielle seguono,
  con il fine corsa del meccanismo (vedi sotto)
- **Progetto** (`.dima3d`): salva quali modelli sono aperti e le modifiche (posizioni, colori, nomi, pezzi
  nascosti o eliminati, vista); all'apertura rilegge i modelli originali e riapplica le modifiche
- **Riduzione delle facce** con anteprima e dimensione prevista del file
- **Export** delle parti visibili, dell'insieme o di un singolo file in WRL, STL, Open Inventor e HSF
- **Misure PDF**: layout in PDF con calibrazione della scala su una distanza nota e misure in mm (Delta X, Delta Y,
  distanza); rendering con [pdfium](https://pdfium.googlesource.com/pdfium/) (`tools\pdfium.dll`, dai binari
  [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries))

### Cerniera a 4 perni: come si scelgono i perni

Finestra **Posizione** → *Rotazione su cerniera* → **4 perni (cofano)**.

- **Cerniera fissa**: la piastra avvitata alla scocca, con due perni.
- **Cerniera mobile**: il braccio avvitato al cofano, con due perni.
- Ogni **biella** unisce un perno fisso a un perno mobile.

1. Parti dalla posizione della matematica (di solito cofano chiuso) e premi *Scegli i 4 perni nella vista*.
2. Su ogni perno clicca **3 punti sul bordo del cerchio, lontani tra loro a formare un triangolo** (non vicini
   e non in fila): dai 3 punti si trova il centro del perno.
3. **L'ordine conta**: prima **F1** e **F2** sulla cerniera fissa, poi **M1** e **M2** sulla cerniera mobile.
   M1 è in cima alla biella che parte da F1, M2 in cima a quella che parte da F2.
4. Con *Usa selezione* indica la **parte che si apre** (cofano + braccio) e, se vuoi che si muovano, le due bielle.
5. Scrivi l'**angolo di apertura del cofano**: le bielle si mettono da sole nella posizione giusta.
   Oltre il fine corsa il cofano si ferma dove arriva il meccanismo e viene segnalato.

Se i perni mobili sono stati cliccati al contrario le bielle risultano incrociate: Dima 3D lo segnala e
*Scambia i perni mobili* lo corregge. Se il cofano si apre dalla parte sbagliata, *Inverti verso*.
Basta una cerniera: il cofano si muove tutto; le bielle dell'altro lato si possono selezionare insieme a
quelle di questo lato se le due cerniere sono allineate. Annulla/Ripeti tengono il conto dell'angolo.

## Compilazione

Serve [Rust](https://rustup.rs) (toolchain MSVC su Windows).

```
cargo build --release
cargo test --release
```

L'eseguibile è `target\release\dima3d.exe` e non richiede installazione: il runtime C è incluso.
I convertitori si compilano a parte: [`tools-src/jt2wrl`](tools-src/jt2wrl/README.md) e
[`tools-src/step2wrl`](tools-src/step2wrl/README.md); vanno in `tools\` accanto all'eseguibile.
Nella stessa cartella va `pdfium.dll` per la finestra Misure PDF: si prende da `bin\` dell'archivio
`pdfium-win-x64.tgz` di [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries/releases)
(versione provata: chromium/8152).

Per preparare lo zip da distribuire (eseguibile, convertitori, licenze, LEGGIMI):

```
powershell -ExecutionPolicy Bypass -File pacchetto.ps1
```

Il risultato è `dist\Dima3D-<versione>.zip`: si estrae dove si vuole e si avvia `dima3d.exe`.

Lo stato dettagliato del progetto e la roadmap sono in [`STATO_PROGETTO.md`](STATO_PROGETTO.md).

## Licenza

Distribuito con licenza MIT: vedi [`LICENSE`](LICENSE). Le licenze e le note di copyright delle librerie
incluse sono nella cartella [`licenses`](licenses): `librerie-rust.txt` per le 216 librerie Rust del
programma (rigenerabile con `python licenses/genera_licenze_rust.py`), più quelle dei convertitori
(OpenCASCADE LGPL 2.1 con eccezione, JTReader e SharpCompress MIT, e le loro dipendenze) e di pdfium
(`pdfium-licenze.txt`: BSD, con le librerie che include).
