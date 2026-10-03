#pragma once

#include "kinnovel/hal/IPower.hpp"
#include <string>
#include <vector>
#include <atomic>
#include <thread>
#include <mutex>

namespace kinnovel::hal {

class LipcPowerManager : public IPower {
public:
    explicit LipcPowerManager(const std::string& pauseFilePath = "/tmp/kinnovel_paused_pids");
    ~LipcPowerManager() override;

    void start(IPowerObserver* observer) override;
    void stop() override;
    bool isSleeping() const override { return m_isSleeping; }
    void handlePowerKey() override;

private:
    void lipcEventLoop();
    void sleepWatchdogLoop();
    void powerKeyLoop(const std::string& devicePath);

    std::vector<int> readPausedPids();
    void writePausedPids(const std::vector<int>& pids);
    std::vector<int> scanFbUsers();

    void suspendLocked();
    void resumeLocked();

    std::string m_pauseFile;
    IPowerObserver* m_observer = nullptr;
    std::atomic<bool> m_isSleeping{false};
    std::atomic<bool> m_stopped{false};

    std::vector<int> m_trackedPids;
    std::mutex m_mutex;
    std::vector<std::thread> m_threads;
    pid_t m_lipcWaitPid = -1;
    double m_lastPowerPressAt = 0.0;
};

} // namespace kinnovel::hal
