# Steinberg ASIO SDK, host part

These files come unchanged from the Steinberg ASIO SDK 2.3.4 (released 2025-10-15,
https://www.steinberg.net/asiosdk). They are the part a host program needs to find and talk to
ASIO drivers. Crucible compiles them through the `asio-sys` crate (`.cargo/config.toml` points
`CPAL_ASIO_DIR` here).

**Licence.** The SDK is offered under Steinberg's proprietary ASIO licence or, alternatively,
the GNU General Public License version 3. Crucible uses it under **GPL version 3**; see
`LICENSE.txt` in this folder (Steinberg's own text) and `LICENSE` at the repository root (the GPL).
Every file here carries Steinberg's notice pointing at that licence.

**What was left out, and why.**

- `common/combase.cpp`, `combase.h`, `dllentry.cpp` and `wxdebug.h`: Microsoft sample code
  ("Copyright (c) 1992 - 1996 Microsoft Corporation. All Rights Reserved.") that does not carry
  Steinberg's licence notice. It is only for writing drivers. It is not in this repository and
  is not compiled into Crucible.
- The rest of the driver-writing side (`asiodrvr.*`, `register.cpp`, `debugmessage.cpp`, the
  `driver` folder), the sample host, the PDF documents and the logo artwork: not needed.

To use the full SDK instead, download it from Steinberg and set `CPAL_ASIO_DIR` to its folder.

ASIO is a trademark and software of Steinberg Media Technologies GmbH.
