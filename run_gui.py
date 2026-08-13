"""Launch the Carousel Canvas desktop UI (development)."""

import multiprocessing

if __name__ == "__main__":
    multiprocessing.freeze_support()
    from app.gui.main import main

    main()
