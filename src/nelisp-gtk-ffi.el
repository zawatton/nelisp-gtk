;;; nelisp-gtk-ffi.el --- Pure-elisp GTK4 bindings via NeLisp FFI  -*- lexical-binding: t; -*-

;; Copyright (C) 2026 zawatton

;; This file is part of nelisp-gtk.
;;
;; This program is free software: you can redistribute it and/or modify
;; it under the terms of the GNU General Public License as published by
;; the Free Software Foundation, either version 3 of the License, or
;; (at your option) any later version.

;;; Commentary:

;; Phase B0 spike skeleton — see docs/design/02-phase-b0-pure-elisp-spike.org.
;;
;; Surface: 14-symbol minimum to open a GtkApplicationWindow with one
;; GtkDrawingArea, register an elisp `draw' callback, and paint a
;; rectangle through Cairo from inside that callback.
;;
;; All bindings here are STUBS pending two prerequisites:
;;
;;   1. `nl-ffi-closure-new' / `nl-ffi-closure-free' (= Phase B0.a, work
;;      lives in nelisp/build-tool/src/eval/ffi.rs, NOT this repo).
;;      Forward calls already work via the shipped `nl-ffi-call'.
;;
;;   2. Resolved shared-object paths.  We do not bake absolute paths;
;;      `nelisp-gtk-ffi--libs' resolves the basename through the
;;      dynamic loader's standard search list at first call.
;;
;; Until (1) lands, the `*-set-draw-func' / `*-signal-connect' wrappers
;; here will `error' to keep callers honest.  Forward-only helpers
;; (= `gtk-window-set-title' etc.) are usable today via `nl-ffi-call'
;; and can be smoke-tested in isolation.

;;; Code:

(eval-when-compile (require 'cl-lib))

;; ---------------------------------------------------------------------
;; Shared object resolution
;; ---------------------------------------------------------------------

(defvar nelisp-gtk-ffi--libs
  '((:gtk     . "libgtk-4.so.1")
    (:gobject . "libgobject-2.0.so.0")
    (:gio     . "libgio-2.0.so.0")
    (:glib    . "libglib-2.0.so.0")
    (:cairo   . "libcairo.so.2"))
  "Mapping from :tag to shared-object SONAME used by the spike.
Resolved via the dynamic linker's normal search path; we never
hard-code absolute paths so that distribution differences (= Debian /
Fedora / NixOS) do not require code edits.")

(defun nelisp-gtk-ffi--lib (tag)
  "Return the shared-object filename associated with TAG."
  (or (alist-get tag nelisp-gtk-ffi--libs)
      (error "nelisp-gtk-ffi: unknown library tag %S" tag)))

;; ---------------------------------------------------------------------
;; Closure FFI guard
;; ---------------------------------------------------------------------

(defun nelisp-gtk-ffi--require-closure ()
  "Signal a structured error when the closure-FFI prerequisite is missing.
Phase B0.a (= `nl-ffi-closure-new' in nelisp/build-tool/src/eval/ffi.rs)
must ship before any signal-connect / draw-func wrapper can work."
  (unless (fboundp 'nl-ffi-closure-new)
    (error
     (concat "nelisp-gtk-ffi: `nl-ffi-closure-new' missing — "
             "ship Phase B0.a in upstream nelisp before retrying. "
             "See docs/design/02-phase-b0-pure-elisp-spike.org §3."))))

;; ---------------------------------------------------------------------
;; Forward-only thin wrappers (work today via shipped `nl-ffi-call')
;; ---------------------------------------------------------------------

(defun nelisp-gtk-ffi-application-new (app-id flags)
  "Wrap `gtk_application_new(APP-ID, FLAGS)' → GtkApplication * pointer."
  (nl-ffi-call (nelisp-gtk-ffi--lib :gtk)
               "gtk_application_new"
               [:pointer :string :uint32]
               app-id flags))

(defun nelisp-gtk-ffi-application-window-new (app-ptr)
  "Wrap `gtk_application_window_new(APP-PTR)' → GtkWindow * pointer."
  (nl-ffi-call (nelisp-gtk-ffi--lib :gtk)
               "gtk_application_window_new"
               [:pointer :pointer]
               app-ptr))

(defun nelisp-gtk-ffi-window-set-title (window-ptr title)
  "Wrap `gtk_window_set_title(WINDOW-PTR, TITLE)'."
  (nl-ffi-call (nelisp-gtk-ffi--lib :gtk)
               "gtk_window_set_title"
               [:void :pointer :string]
               window-ptr title))

(defun nelisp-gtk-ffi-window-set-default-size (window-ptr w h)
  "Wrap `gtk_window_set_default_size(WINDOW-PTR, W, H)'."
  (nl-ffi-call (nelisp-gtk-ffi--lib :gtk)
               "gtk_window_set_default_size"
               [:void :pointer :sint32 :sint32]
               window-ptr w h))

(defun nelisp-gtk-ffi-window-set-child (window-ptr child-ptr)
  "Wrap `gtk_window_set_child(WINDOW-PTR, CHILD-PTR)'."
  (nl-ffi-call (nelisp-gtk-ffi--lib :gtk)
               "gtk_window_set_child"
               [:void :pointer :pointer]
               window-ptr child-ptr))

(defun nelisp-gtk-ffi-window-present (window-ptr)
  "Wrap `gtk_window_present(WINDOW-PTR)'."
  (nl-ffi-call (nelisp-gtk-ffi--lib :gtk)
               "gtk_window_present"
               [:void :pointer]
               window-ptr))

(defun nelisp-gtk-ffi-drawing-area-new ()
  "Wrap `gtk_drawing_area_new()' → GtkDrawingArea * pointer."
  (nl-ffi-call (nelisp-gtk-ffi--lib :gtk)
               "gtk_drawing_area_new"
               [:pointer]))

(defun nelisp-gtk-ffi-object-unref (object-ptr)
  "Wrap `g_object_unref(OBJECT-PTR)'."
  (nl-ffi-call (nelisp-gtk-ffi--lib :gobject)
               "g_object_unref"
               [:void :pointer]
               object-ptr))

(defun nelisp-gtk-ffi-cairo-set-source-rgb (cr r g b)
  "Wrap `cairo_set_source_rgb(CR, R, G, B)'.  R/G/B in [0.0, 1.0]."
  (nl-ffi-call (nelisp-gtk-ffi--lib :cairo)
               "cairo_set_source_rgb"
               [:void :pointer :double :double :double]
               cr r g b))

(defun nelisp-gtk-ffi-cairo-rectangle (cr x y w h)
  "Wrap `cairo_rectangle(CR, X, Y, W, H)'."
  (nl-ffi-call (nelisp-gtk-ffi--lib :cairo)
               "cairo_rectangle"
               [:void :pointer :double :double :double :double]
               cr x y w h))

(defun nelisp-gtk-ffi-cairo-fill (cr)
  "Wrap `cairo_fill(CR)'."
  (nl-ffi-call (nelisp-gtk-ffi--lib :cairo)
               "cairo_fill"
               [:void :pointer]
               cr))

(defun nelisp-gtk-ffi-application-run (app-ptr argc argv-ptr)
  "Wrap `g_application_run(APP-PTR, ARGC, ARGV-PTR)' → int exit code.
Blocks the calling thread until the GtkApplication exits.  ARGV-PTR is
a `:pointer' to a `char **' array (= nil works in the spike since we
pass argc=0)."
  (nl-ffi-call (nelisp-gtk-ffi--lib :gio)
               "g_application_run"
               [:sint32 :pointer :sint32 :pointer]
               app-ptr argc argv-ptr))

;; ---------------------------------------------------------------------
;; Closure-dependent wrappers (= STUB until Phase B0.a lands)
;; ---------------------------------------------------------------------

(defun nelisp-gtk-ffi-signal-connect (instance-ptr signal-name callback)
  "Connect CALLBACK (= elisp callable) to SIGNAL-NAME on INSTANCE-PTR.
Returns the connection handler id as an integer.

STUB: needs `nl-ffi-closure-new' to convert CALLBACK to a C function
pointer.  See `docs/design/02-phase-b0-pure-elisp-spike.org' §3."
  (nelisp-gtk-ffi--require-closure)
  (let* ((sig (vector :sint64 :pointer :pointer :pointer))
         (cb-ptr (nl-ffi-closure-new sig callback)))
    (nl-ffi-call (nelisp-gtk-ffi--lib :gobject)
                 "g_signal_connect_data"
                 [:uint64 :pointer :string :pointer :pointer :pointer :uint32]
                 instance-ptr signal-name cb-ptr 0 0 0)))

(defun nelisp-gtk-ffi-drawing-area-set-draw-func (area-ptr draw-cb)
  "Register DRAW-CB (= elisp callable) as the `draw' func for AREA-PTR.
DRAW-CB is invoked as (DRAW-CB AREA-PTR CR-PTR WIDTH HEIGHT).

STUB: needs `nl-ffi-closure-new'.  See doc §3 for the prereq."
  (nelisp-gtk-ffi--require-closure)
  (let* ((sig (vector :void :pointer :pointer :sint32 :sint32 :pointer))
         (cb-ptr (nl-ffi-closure-new sig draw-cb)))
    (nl-ffi-call (nelisp-gtk-ffi--lib :gtk)
                 "gtk_drawing_area_set_draw_func"
                 [:void :pointer :pointer :pointer :pointer]
                 area-ptr cb-ptr 0 0)))

(provide 'nelisp-gtk-ffi)

;;; nelisp-gtk-ffi.el ends here
