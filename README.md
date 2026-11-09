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
- **Riduzione delle facce** con anteprima e dimensione prevista del file
- **Export** delle parti visibili, dell'insieme o di un singolo file in WRL, STL, Open Inventor e HSF

## Compilazione

Serve [Rust](https://rustup.rs) (toolchain MSVC su Windows).

```
cargo build --release
cargo test --release
```

L'eseguibile è `target\release\dima3d.exe` e non richiede installazione: il runtime C è incluso.
I convertitori si compilano a parte: [`tools-src/jt2wrl`](tools-src/jt2wrl/README.md) e
[`tools-src/step2wrl`](tools-src/step2wrl/README.md); vanno in `tools\` accanto all'eseguibile.

Per preparare lo zip da distribuire (eseguibile, convertitori, licenze, LEGGIMI):

```
powershell -ExecutionPolicy Bypass -File pacchetto.ps1
```

Il risultato è `dist\Dima3D-<versione>.zip`: si estrae dove si vuole e si avvia `dima3d.exe`.

Lo stato dettagliato del progetto e la roadmap sono in [`STATO_PROGETTO.md`](STATO_PROGETTO.md).

## Licenza

Distribuito con licenza MIT: vedi [`LICENSE`](LICENSE). Le licenze e le note di copyright delle librerie
incluse sono nella cartella [`licenses`](licenses): `librerie-rust.txt` per le 203 librerie Rust del
programma (rigenerabile con `python licenses/genera_licenze_rust.py`), più quelle dei convertitori
(OpenCASCADE LGPL 2.1 con eccezione, JTReader e SharpCompress MIT, e le loro dipendenze).
