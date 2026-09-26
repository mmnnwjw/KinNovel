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
            mock_kill.assert_any_call(10001, signal.SIGCONT)
            mock_kill.assert_any_call(10002, signal.SIGCONT)

            # Redundant suspend should be no-op
            app.screen.input.device.ungrab.reset_mock()
            mgr.handle_suspend()
            app.screen.input.device.ungrab.assert_not_called()

            # 2. Trigger resume
            mgr.handle_resume()
            self.assertFalse(mgr.is_sleeping)
            # Verify sleep delay
            mock_sleep.assert_called_with(0.35)
            # Verify SIGSTOP was sent to pids
            mock_kill.assert_any_call(10001, signal.SIGSTOP)
            mock_kill.assert_any_call(10002, signal.SIGSTOP)
            # Verify grab was called
            app.screen.input.device.grab.assert_called_once()
            # Verify context.show was called with is_flashing=True
            app.context.show.assert_called_once_with(is_flashing=True)

            # Redundant resume should be no-op
            app.context.show.reset_mock()
            mgr.handle_resume()
            app.context.show.assert_not_called()

    def test_power_key_press_when_active(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)

        with patch.object(mgr, "_suspend_locked") as mock_suspend, \
             patch("shutil.which", return_value="/usr/bin/lipc-set-prop"), \
             patch("subprocess.run") as mock_run:
            mgr.handle_power_key()
            mock_suspend.assert_called_once()
            mock_run.assert_called_once_with(
                ["lipc-set-prop", "com.lab126.powerd", "state", "screenSaver"],
                timeout=2.0,
                stdout=unittest.mock.ANY,
                stderr=unittest.mock.ANY,
            )

    def test_power_key_press_when_sleeping(self):
        app = MagicMock()
        mgr = PowerManager(app, pause_file=self.pause_file)
        mgr.is_sleeping = True

        # When lipc-wait-event is available, it should wait for system event
        with patch.object(mgr, "_resume_locked") as mock_resume, \
             patch("shutil.which", return_value="/usr/bin/lipc-wait-event"):
            mgr.handle_power_key()
            mock_resume.assert_not_called()

        # When lipc-wait-event is not available, it should resume directly
        with patch.object(mgr, "_resume_locked") as mock_resume, \
             patch("shutil.which", return_value=None):
            mgr.handle_power_key()
            mock_resume.assert_called_once()

    def test_stop_cleans_up_if_sleeping(self):
        app = MagicMock()
        test_pids = [10001]
        write_paused_pids(test_pids, self.pause_file)

        mgr = PowerManager(app, pause_file=self.pause_file)
        mgr.is_sleeping = True

        with patch("os.kill") as mock_kill:
            mgr.stop()
            self.assertTrue(mgr._stopped)
            mock_kill.assert_called_once_with(10001, signal.SIGCONT)


if __name__ == "__main__":
    unittest.main()
