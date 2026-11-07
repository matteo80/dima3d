# Gestore WRL

Applicazione desktop per Windows, scritta in Rust, per aprire, ispezionare, modificare ed esportare file
VRML (`.wrl`) di grandi dimensioni (centinaia di MB), come le matematiche di scocche e componenti
automobilistici.

## Funzioni

- Lettura veloce di VRML 2.0 e VRML 1.0 (file mappati in memoria, analisi in parallelo)
- Vista 3D con wgpu: antialiasing, illuminazione che segue la vista, spigoli vivi dalle normali del file,
  sfondo uniforme o sfumato
- Navigazione in stile OpenInventor, viste standard in coordinate veicolo, cubo di orientamento cliccabile
- Più file nella stessa scena, con conversione delle unità (millimetri ↔ metri)
- Albero della scena con ricerca, raggruppamento automatico dei pezzi connessi, colori, rinomina,
  nascondi/isola/elimina, Annulla/Ripeti
- Proprietà del pezzo selezionato, misura della distanza tra due punti, piano di sezione
- Export in WRL delle sole parti visibili, dell'insieme o di un singolo file, senza perdere precisione

## Compilazione

Serve [Rust](https://rustup.rs) (toolchain MSVC su Windows).

```
cargo build --release
```

L'eseguibile è `target\release\gestore-wrl.exe` e non richiede installazione: il runtime C è incluso.

```
cargo test --release
```

Lo stato dettagliato del progetto e la roadmap sono in [`STATO_PROGETTO.md`](STATO_PROGETTO.md).

## Licenza

Distribuito con licenza MIT: vedi [`LICENSE`](LICENSE).
