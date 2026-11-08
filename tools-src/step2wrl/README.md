# step2wrl — convertitore STEP → VRML 2.0

Programma separato usato dal Gestore WRL per aprire i file `.stp`/`.step`: il Gestore lo avvia
senza finestra, legge l'avanzamento da stdout e apre il `.wrl` prodotto. Se uno STEP danneggiato
fa terminare il convertitore, il Gestore resta aperto e mostra l'errore.

Usa [OpenCASCADE](https://github.com/Open-Cascade-SAS/OCCT) 8.0.1, la stessa libreria con cui
FreeCAD legge gli STEP: assieme, nomi e colori (XCAF), tassellazione della geometria esatta con
`BRepMesh_IncrementalMesh`, come le "Opzioni di mesh" Standard di FreeCAD.

## Uso

```
step2wrl <entrata.stp> <uscita.wrl> [--deviazione 0.1] [--angolo 30] [--relativa]
```

- `--deviazione`: distanza massima tra la mesh e la superficie vera, in mm (predefinita 0,1).
  Con `--relativa` è una frazione della dimensione di ogni faccia.
- `--angolo`: deviazione angolare massima in gradi (predefinita 30).

Stdout: righe `PROGRESS <0..1> <testo>`, alla fine `DONE <pezzi> <triangoli> <saltati>`.
Stderr: avvisi (`Avviso: ...`) ed errori. Codice di uscita: 0 riuscito, 1 argomenti, 2 errore di
lettura, 3 nessun pezzo con superfici, 4 errore di scrittura.

Il WRL ha un `Group` per ogni assieme e uno `Shape` per ogni pezzo (un gruppo di `Shape` se le
facce del pezzo hanno colori diversi), con le coordinate già trasformate, in mm, e le normali
della tassellazione (spigoli vivi tra le facce).

## Compilazione

Servono Visual Studio 2022 con gli strumenti C++ e Python (per copiare le DLL).

1. Scaricare il pacchetto binario di OpenCASCADE dalla
   [release V8.0.1](https://github.com/Open-Cascade-SAS/OCCT/releases/tag/V8.0.1)
   (`occt-combined-release-no-pch.zip`) ed estrarlo, compreso lo zip interno, in
   `%USERPROFILE%\occt-8.0.1` (cartelle `opencascade-8.0.1-vc14-64` e `3rdparty-vc14-64`).
   Un'altra posizione si indica con la variabile `OCCT`.
2. `tools-src\step2wrl\build.cmd`

Il risultato è `target\release\tools\step2wrl\`: `step2wrl.exe` con le DLL di OpenCASCADE, delle
librerie che queste richiedono e del runtime di Visual C++ (circa 65 MB). La cartella va copiata
intera dentro `tools` accanto a `gestore-wrl.exe` (lo fa `pacchetto.ps1`).

## Licenze

`main.cpp` fa parte del Gestore WRL (licenza MIT). Le librerie sono usate come DLL non
modificate, in un programma separato dal Gestore; i testi delle licenze sono nella cartella
`licenses` del repository e del pacchetto.

| Libreria | Licenza | Sorgenti |
|---|---|---|
| OpenCASCADE 8.0.1 | LGPL 2.1 con eccezione OCCT | https://github.com/Open-Cascade-SAS/OCCT/tree/V8.0.1 |
| FFmpeg 3.3.4 (avcodec, avformat, avutil, swscale), richiesto da `TKService` | LGPL 2.1 | https://ffmpeg.org/releases/ffmpeg-3.3.4.tar.xz |
| FreeImage 3.18.0 | FreeImage Public License | https://freeimage.sourceforge.io |
| FreeType 2.13.3 | FreeType License | https://freetype.org |
| oneTBB 2021.13 | Apache 2.0 | https://github.com/uxlfoundation/oneTBB |
| jemalloc | BSD 2 clausole | https://github.com/jemalloc/jemalloc |
| OpenVR 1.14.15 | BSD 3 clausole | https://github.com/ValveSoftware/openvr |
| Runtime di Visual C++ | file ridistribuibili di Microsoft Visual Studio | — |

Le DLL di OpenCASCADE e FFmpeg possono essere sostituite con altre versioni compatibili, come
richiede la LGPL.
