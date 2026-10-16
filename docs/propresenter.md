# ProPresenter integration

StagePilot controls one reusable ProPresenter timer through ProPresenter's local HTTP API. When a song cue arrives, StagePilot stops the timer, configures it as a Countdown Timer with the song duration, resets it, verifies the saved duration, and then starts it.

## Required ProPresenter setup

1. Open ProPresenter **Settings → Network** and enable the API.
2. Note the configured API port. StagePilot defaults to `1025` but does not assume that value.
3. Create a countdown timer named `Song Countdown`, or select another detected countdown timer in StagePilot.
4. Link the timer to the desired ProPresenter stage layout.

## StagePilot configuration

The production setup panel persistently stores:

- Host
- Port
- Timer name
- Request timeout
- Connection test
- Timer rediscovery

Save the connection settings in the panel. Saving automatically enables real
ProPresenter output and connects right away, with no restart; no simulated
output choice is shown in the production panel. Switching between simulated and
real output is refused while the timer is running. Environment overrides remain available for
development or unattended startup:

```dotenv
STAGEPILOT_TIMER_OUTPUT=propresenter
STAGEPILOT_PROPRESENTER_HOST=127.0.0.1
STAGEPILOT_PROPRESENTER_PORT=1025
STAGEPILOT_PROPRESENTER_TIMER_NAME=Song Countdown
STAGEPILOT_PROPRESENTER_TIMEOUT_SECONDS=3
STAGEPILOT_PROPRESENTER_RECONNECT_INITIAL_SECONDS=1
STAGEPILOT_PROPRESENTER_RECONNECT_MAX_SECONDS=30
STAGEPILOT_PROPRESENTER_HEALTH_CHECK_SECONDS=10
```

## Timer sequence

For `song.started` and `song.restarted`, StagePilot performs:

1. Stop
2. Set countdown duration
3. Reset
4. Start

`timer.stop_requested` stops the real ProPresenter timer while retaining its
configured duration. **Reset Position** performs a full reset sequence:

1. Stop
2. Set countdown duration to `0:00`
3. Reset

The dashboard records the start immediately before the ProPresenter start
request and advances the displayed remaining time on the first fractional
second. This keeps StagePilot's main countdown aligned with ProPresenter rather
than displaying the previous whole second for nearly one extra second.

## Recovery

StagePilot periodically probes ProPresenter. If ProPresenter is offline at startup or restarts later, the plugin retries with capped exponential backoff. Timer identities are rediscovered by visible name, and failed timer operations clear the cached timer before one immediate retry.

## macOS diagnostic log

Packaged macOS builds save backend diagnostics here:

`~/Library/Logs/org.stagepilot.desktop/stagepilot-backend.log`

In Finder, choose **Go → Go to Folder**, paste that path, and press Return. Fully quit and reopen StagePilot before reproducing a problem so the latest session appears at the end of the file. Review the file before sharing it; it should not contain saved secrets, but service and timer names may appear.

The dashboard distinguishes:

- API unreachable
- API connected but configured timer missing
- Timer found and ready
- Timer command failure
