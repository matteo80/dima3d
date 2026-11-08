# Gestore WRL

Applicazione desktop per Windows, scritta in Rust, per aprire, ispezionare, modificare ed esportare file
VRML (`.wrl`) di grandi dimensioni (centinaia di MB), come le matematiche di scocche e componenti
automobilistici.

## Funzioni

- Lettura veloce di VRML 2.0 e VRML 1.0 (file mappati in memoria, analisi in parallelo)
- Apertura dei file JT 8, 9 e 10 (geometria tassellata, assiemi in più file) con il convertitore
  `tools\jt2wrl.exe`: vedi [`tools-src/jt2wrl`](tools-src/jt2wrl/README.md)
- Vista 3D con wgpu: antialiasing, illuminazione che segue la vista, spigoli vivi dalle normali del file,
  sfondo uniforme o sfumato
- Navigazione in stile OpenInventor, viste standard in coordinate veicolo, cubo di orientamento cliccabile
- Più file nella stessa scena, con conversione delle unità (millimetri ↔ metri)
- Albero della scena con ricerca, raggruppamento automatico dei pezzi connessi, colori, rinomina,
  nascondi/isola/elimina, Annulla/Ripeti
- Proprietà del pezzo selezionato, misura della distanza tra due punti, piano di sezione
- Export delle sole parti visibili, dell'insieme o di un singolo file in WRL, STL e Open Inventor (`.iv`,
  testo o binario)
- Apertura dei file STL (binari e di testo)
- Riduzione delle facce con anteprima e dimensione prevista del file
- Colori casuali per pezzo, pipetta per copiare il colore di un pezzo, colori salvati

## Compilazione

Serve [Rust](https://rustup.rs) (toolchain MSVC su Windows).

```
cargo build --release
```

L'eseguibile è `target\release\gestore-wrl.exe` e non richiede installazione: il runtime C è incluso.
Per aprire i file JT serve anche `tools\jt2wrl.exe` accanto all'eseguibile (compilazione in
[`tools-src/jt2wrl`](tools-src/jt2wrl/README.md)).

```
cargo test --release
```

Per preparare lo zip da distribuire (eseguibile, convertitore JT, licenze, LEGGIMI):

```
powershell -ExecutionPolicy Bypass -File pacchetto.ps1
```

Il risultato è `dist\Gestore-WRL-<versione>.zip`: si estrae dove si vuole e si avvia `gestore-wrl.exe`.

Lo stato dettagliato del progetto e la roadmap sono in [`STATO_PROGETTO.md`](STATO_PROGETTO.md).

## Licenza

Distribuito con licenza MIT: vedi [`LICENSE`](LICENSE). Il convertitore JT include JTReader e
SharpCompress, anch'essi MIT: licenze in [`licenses`](licenses).
