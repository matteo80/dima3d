# Piano: guida di Dima 3D in italiano e inglese

Stato: **proposta** (2026-11-21), da avviare. Al posto di francese e spagnolo (rimandati: stima ~6 ore,
bandierine già pronte in `src/ui/flags.rs`).

## Scelte proposte (da confermare all'inizio del lavoro)
- **Formato**: pagine HTML aperte nel browser, `guida/it/index.html` e `guida/en/index.html` con le immagini in
  `guida/<lingua>/img/`. Ricerca con Ctrl+F, stampa/PDF dal browser, nessun peso sull'exe.
- **Apertura**: pulsante «Guida» (icona punto di domanda) accanto a Info nella barra, e tasto F1; apre la pagina
  nella lingua in uso. Nella finestra Info un collegamento alla guida.
- **Contenuto per procedure**, non pulsante per pulsante: capitoli brevi (scopo, passi numerati, 1–3 miniature,
  avvertenze). I suggerimenti sui pulsanti restano la documentazione di dettaglio.
- **Miniature catturate in automatico** dal programma (rigenerabili a ogni cambio dell'interfaccia), in tutte e due
  le lingue, solo con file di esempio inventati (il repository è pubblico: niente nomi reali di aziende o file).

## Capitoli (bozza, da rivedere con l'utente)
1. Primi passi: aprire un file, muovere la vista (rotazione, zoom, viste standard, cubo), tema e lingua
2. Albero della scena: selezionare, cercare, nascondere, isolare, gruppi, colori, pipetta
3. Aprire più modelli e formati (WRL, STEP, JT, STL, HSF, IV), unità e raggruppamento automatico
4. Progetti .dima3d: salvare e riaprire la scena con le modifiche
5. Posizione: spostare e ruotare (X Y Z, A B C), cerniere
6. Applicare un offset; calcolare l'offset (tra due oggetti, tra due CONST BASE, Inverti)
7. Allineare: su punti, su misure (Veloce/Precisa)
8. Misure: strumento Misura, misure salvate, Misura dell'oggetto e report
9. Cabina di verniciatura: misure, aperture, scritta, centro cabina
10. Robot: robot generici con le coordinate del controllore
11. Programma del robot (.TIS/.tip): traiettoria, modifica delle righe
12. Misure da PDF: calibrare la scala e misurare
13. Riduci facce ed export (WRL, STL, IV, HSF; opzioni)
14. Finestre agganciabili, barra compatta, preferenze (menu Vista), aggiornamenti
15. Domande frequenti e problemi comuni (file senza superfici, file mancanti nel progetto…)

## Step
1. **Scena dimostrativa** (~1 h): in `esempi/` file inventati e piccoli: una dima semplice generata (WRL), un
   secondo modello per l'allineamento, un programma robot di prova, un layout PDF di prova, un progetto .dima3d
   che li unisce con cabina e robot. Controllo privacy (hook) prima del commit.
2. **Cattura delle miniature** (~2 h): avvio speciale `dima3d --cattura <cartella> --lingua it|en` che apre la
   scena dimostrativa, esegue una sequenza di stati (finestre aperte, menu, selezioni) e salva le immagini con
   `egui::ViewportCommand::Screenshot` (evento `egui::Event::Screenshot`), ritagliate sulla zona utile e ridotte
   a miniatura + immagine grande. Uno script `guida.ps1` le rigenera in tutte e due le lingue.
3. **Struttura e apertura** (~1 h): modello HTML/CSS unico (indice laterale, capitoli, miniature cliccabili, tema
   chiaro/scuro), pulsante «Guida» e F1, ricerca della cartella `guida` accanto all'exe (e nel repository quando
   si avvia con cargo run); `pacchetto.ps1` la copia nello zip e nel setup.
4. **Testi in italiano** (~3 h, a blocchi di capitoli): scritti dalle funzioni vere (codice e suggerimenti),
   rilettura dell'utente capitolo per capitolo.
5. **Versione inglese** (~1,5 h): con i termini del glossario di `src/i18n/en.txt`.
6. **Controlli** (~1 h): test che verifica che ogni immagine citata esista e che le due lingue abbiano gli stessi
   capitoli; controllo privacy dei testi (anche `pubblica.ps1` controlla i file di testo del pacchetto); prova a
   schermo; beta.

**Totale stimato: ~8–9 ore, in 3–4 sessioni.** Ordine consigliato: 1 → 2 → 3 (si vede subito una guida con
immagini vere), poi 4 e 5 a blocchi, infine 6 e una beta.

## Rischi
- Le miniature automatiche richiedono che ogni stato sia raggiungibile da codice (finestre, menu aperti): i menu a
  tendina di egui potrebbero richiedere eventi simulati; in alternativa per pochi casi si catturano a mano.
- La guida va tenuta aggiornata: a ogni nuova funzione, capitolo e miniature (le miniature si rigenerano da sole).
