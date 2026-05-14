;;; hello-window.el --- Phase B0 spike PoC entry point  -*- lexical-binding: t; -*-

;; Copyright (C) 2026 zawatton

;; This file is part of nelisp-gtk.
;;
;; This program is free software: you can redistribute it and/or modify
;; it under the terms of the GNU General Public License as published by
;; the Free Software Foundation, either version 3 of the License, or
;; (at your option) any later version.

;;; Commentary:

;; Phase B0 spike — see ../docs/design/02-phase-b0-pure-elisp-spike.org.
;;
;; Goal: open a GtkApplicationWindow + GtkDrawingArea, register an
;; elisp `draw' callback that paints a red rectangle via Cairo, exit
;; cleanly when the user closes the window — all without compiled Rust.
;;
;; Status: SCAFFOLDING.  Will not run until Phase B0.a — the
;; `nelisp-gtk-make-closure' builtin in this repo's Rust shim — lands.
;; Forward-only calls (window creation, title, size) work today and
;; can be smoke-tested by commenting out the signal-connect line.

;;; Code:

(require 'nelisp-gtk-ffi)

(defun hello-window--draw-cb (_area cr width height)
  "Paint a solid red rectangle covering the drawing area.
Called by GTK from inside its main loop; AREA, CR, WIDTH, HEIGHT
are the canonical `draw_func' arguments per GTK4 docs."
  (nelisp-gtk-ffi-cairo-set-source-rgb cr 0.85 0.20 0.20)
  (nelisp-gtk-ffi-cairo-rectangle cr 0.0 0.0 (float width) (float height))
  (nelisp-gtk-ffi-cairo-fill cr))

(defun hello-window--on-activate (app)
  "GtkApplication `activate' handler — build window + drawing area."
  (let* ((win  (nelisp-gtk-ffi-application-window-new app))
         (area (nelisp-gtk-ffi-drawing-area-new)))
    (nelisp-gtk-ffi-window-set-title win "nelisp-gtk B0 spike")
    (nelisp-gtk-ffi-window-set-default-size win 480 320)
    (nelisp-gtk-ffi-drawing-area-set-draw-func area #'hello-window--draw-cb)
    (nelisp-gtk-ffi-window-set-child win area)
    (nelisp-gtk-ffi-window-present win)))

(defun hello-window-run ()
  "Spike entry point — start the GTK4 application, block until quit."
  (let ((app (nelisp-gtk-ffi-application-new "io.zawatton.nelisp-gtk.spike" 0)))
    (nelisp-gtk-ffi-signal-connect app "activate" #'hello-window--on-activate)
    (unwind-protect
        (nelisp-gtk-ffi-application-run app 0 0)
      (nelisp-gtk-ffi-object-unref app))))

(when noninteractive
  (hello-window-run))

(provide 'hello-window)

;;; hello-window.el ends here
