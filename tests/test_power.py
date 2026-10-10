import os
import signal
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

from kinnovel.power import (
    PowerManager,
    read_paused_pids,
    write_paused_pids,
)

SIGCONT = getattr(signal, "SIGCONT", 18)
SIGSTOP = getattr(signal, "SIGSTOP", 19)


class TestPowerManagement(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.pause_file = Path(self.temp_dir.name) / "paused_pids"

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_read_and_write_paused_pids(self):
        write_paused_pids([123, 456, 789], self.pause_file)
        pids = read_paused_pids(self.pause_file)
        self.assertEqual(pids, [123, 456, 789])

    def test_read_paused_pids_missing_file(self):
        missing = Path(self.temp_dir.name) / "missing"
        self.assertEqual(read_paused_pids(missing), [])

    def test_suspend_and_resume_flow(self):
        # Create mock app
        app = MagicMock()
        app.screen.input.device = MagicMock()
        app.context = MagicMock()
        log_messages = []
        app.log = lambda msg: log_messages.append(msg)

        # Write fake paused pids
        test_pids = [10001, 10002]
        write_paused_pids(test_pids, self.pause_file)

        mgr = PowerManager(app, pause_file=self.pause_file)
        self.assertFalse(mgr.is_sleeping)

        with patch("os.kill") as mock_kill, patch("time.sleep") as mock_sleep:
            # 1. Trigger suspend
            mgr.handle_suspend()
            self.assertTrue(mgr.is_sleeping)
            # Verify ungrab was called
            app.screen.input.device.ungrab.assert_called_once()
            # Verify SIGCONT was sent to pids
            mock_kill.assert_any_call(10001, SIGCONT)
            mock_kill.assert_any_call(10002, SIGCONT)

            # Redundant suspend should be no-op
            app.screen.input.device.ungrab.reset_mock()
            mgr.handle_suspend()
            app.screen.input.device.ungrab.assert_not_called()

            # 2. Trigger resume
            mgr.handle_resume()
            self.assertFalse(mgr.is_sleeping)
            # Verify sleep delay
            mock_sleep.assert_any_call(0.35)
            # Verify SIGSTOP was sent to pids
            mock_kill.assert_any_call(10001, SIGSTOP)
            mock_kill.assert_any_call(10002, SIGSTOP)
            # Verify grab was called
            app.screen.input.device.grab.assert_called_once()
            # 唤醒重绘必须回到主循环线程
            post_callback = app.context.post.call_args.args[0]
            post_callback()
            app.context.show.assert_called_once_with(is_flashing=True, force=True)
            self.assertEqual(app.screen.input.reset_gesture_state.call_count, 2)

            # Redundant resume should be no-op
            app.context.show.reset_mock()
            mgr.handle_resume()
            app.context.show.assert_not_called()

    def test_suspend_calls_api_suspend_and_resume_calls_api_resume(self):
        app = MagicMock()
        app.screen.input.device = MagicMock()
        app.context = MagicMock()

        mgr = PowerManager(app, pause_file=self.pause_file)

        with patch("os.kill"), patch("time.sleep"):
            mgr.handle_suspend()
            app.api.suspend.assert_called_once()
            app.api.resume.assert_not_called()

            mgr.handle_resume()
            app.api.resume.assert_called_once()

    def test_suspend_tolerates_app_without_api(self):
        """Fake apps used elsewhere in tests may not have an `api` attribute;
        suspend/resume must not raise in that case."""
        class BareApp:
            pass

        app = BareApp()
        app.screen = MagicMock()
        app.context = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)

        with patch("os.kill"), patch("time.sleep"):
            mgr.handle_suspend()  # should not raise
            mgr.handle_resume()  # should not raise

    def test_suspend_logs_but_does_not_raise_when_api_suspend_fails(self):
        app = MagicMock()
        app.screen.input.device = MagicMock()
        app.context = MagicMock()
        app.api.suspend.side_effect = RuntimeError("boom")
        log_messages = []
        app.log = lambda msg: log_messages.append(msg)

        mgr = PowerManager(app, pause_file=self.pause_file)
        with patch("os.kill"), patch("time.sleep"):
            mgr.handle_suspend()  # should not raise
        self.assertTrue(any("boom" in msg for msg in log_messages))

    def test_power_key_press_when_active(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)

        with patch.object(mgr, "_suspend_locked") as mock_suspend, \
             patch("subprocess.run") as mock_run:
            mgr.handle_power_key()
            mock_suspend.assert_called_once()
            mock_run.assert_not_called()

    def test_power_key_press_when_sleeping_state_is_active(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)
        mgr.is_sleeping = True

        with patch.object(mgr, "_resume_locked") as mock_resume, \
             patch("shutil.which", return_value="/usr/bin/lipc-get-prop"), \
             patch.object(mgr, "_read_powerd_state", return_value="active"):
            mgr.handle_power_key()
            mock_resume.assert_called_once()

    def test_power_key_press_when_sleeping_waits_in_screen_saver(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)
        mgr.is_sleeping = True

        with patch.object(mgr, "_resume_locked") as mock_resume, \
             patch("shutil.which", return_value="/usr/bin/lipc-get-prop"), \
             patch.object(mgr, "_read_powerd_state", return_value="screenSaver"):
            mgr.handle_power_key()
            mock_resume.assert_not_called()

    def test_power_key_press_when_sleeping_query_failure_waits(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)
        mgr.is_sleeping = True

        with patch.object(mgr, "_resume_locked") as mock_resume, \
             patch("shutil.which", return_value="/usr/bin/lipc-get-prop"), \
             patch.object(mgr, "_read_powerd_state", return_value=None):
            mgr.handle_power_key()
            mock_resume.assert_not_called()

    def test_sleep_watchdog_first_active_waits_without_injection(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)
        mgr.is_sleeping = True

        with patch.object(mgr, "_read_powerd_state", return_value="active"), \
             patch("subprocess.run") as mock_run:
            streak = mgr._sleep_watchdog_check(0)

        self.assertEqual(streak, 1)
        mock_run.assert_not_called()

    def test_sleep_watchdog_second_active_resumes(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)
        mgr.is_sleeping = True

        with patch.object(mgr, "_read_powerd_state", return_value="active"), \
             patch.object(mgr, "handle_resume") as mock_resume, \
             patch("subprocess.run") as mock_run:
            streak = mgr._sleep_watchdog_check(1)

        self.assertEqual(streak, 0)
        mock_resume.assert_called_once()
        mock_run.assert_not_called()

    def test_sleep_watchdog_poll_interval_backs_off_after_fast_window(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)

        self.assertEqual(
            mgr._sleep_watchdog_poll_interval(0.0), mgr._WATCHDOG_FAST_INTERVAL)
        self.assertEqual(
            mgr._sleep_watchdog_poll_interval(mgr._WATCHDOG_FAST_WINDOW - 0.01),
            mgr._WATCHDOG_FAST_INTERVAL)
        self.assertEqual(
            mgr._sleep_watchdog_poll_interval(mgr._WATCHDOG_FAST_WINDOW),
            mgr._WATCHDOG_SLOW_INTERVAL)
        self.assertEqual(
            mgr._sleep_watchdog_poll_interval(mgr._WATCHDOG_FAST_WINDOW + 100),
            mgr._WATCHDOG_SLOW_INTERVAL)

    def test_sleep_watchdog_loop_uses_backed_off_interval_while_sleeping(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)
        mgr.is_sleeping = True

        sleep_calls = []
        iterations = {"count": 0}

        def fake_sleep(seconds):
            sleep_calls.append(seconds)
            iterations["count"] += 1
            if iterations["count"] >= 3:
                mgr._stopped = True

        fake_monotonic_values = iter([0.0, 0.0, 40.0, 40.0, 40.0, 40.0])

        def fake_monotonic():
            return next(fake_monotonic_values, 40.0)

        with patch("time.sleep", side_effect=fake_sleep), \
             patch("time.monotonic", side_effect=fake_monotonic), \
             patch.object(mgr, "_read_powerd_state", return_value="screenSaver"):
            mgr._sleep_watchdog_loop()

        # First poll happens right after sleep starts (fast interval), a
        # later poll (after the fast window elapses) backs off to the slow
        # interval - this is what keeps lipc-get-prop from forking every 2s
        # for as long as the device stays asleep.
        self.assertIn(mgr._WATCHDOG_FAST_INTERVAL, sleep_calls)
        self.assertIn(mgr._WATCHDOG_SLOW_INTERVAL, sleep_calls)

    def test_stop_cleans_up_if_sleeping(self):
        app = MagicMock()
        test_pids = [10001]
        write_paused_pids(test_pids, self.pause_file)

        mgr = PowerManager(app, pause_file=self.pause_file)
        mgr.is_sleeping = True

        with patch("os.kill") as mock_kill:
            mgr.stop()
            self.assertTrue(mgr._stopped)
            mock_kill.assert_called_once_with(10001, SIGCONT)

    def test_stop_terminates_waits_and_joins_threads(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)
        lipc_proc = MagicMock()
        lipc_proc.poll.return_value = None
        listener = MagicMock()
        mgr._lipc_proc = lipc_proc
        mgr._threads = [listener]

        mgr.stop()

        lipc_proc.terminate.assert_called_once()
        lipc_proc.wait.assert_called_once_with(timeout=2)
        listener.join.assert_called_once_with(timeout=1.0)


if __name__ == "__main__":
    unittest.main()
