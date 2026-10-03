#include "LipcPowerManager.hpp"
#include "kinnovel/core/Logger.hpp"

#include <fcntl.h>
#include <unistd.h>
#include <signal.h>
#include <sys/wait.h>
#include <dirent.h>
#include <glob.h>
#include <linux/input.h>
#include <fstream>
#include <sstream>
#include <chrono>

namespace kinnovel::hal {

LipcPowerManager::LipcPowerManager(const std::string& pauseFilePath)
    : m_pauseFile(pauseFilePath) {
}

LipcPowerManager::~LipcPowerManager() {
    stop();
}

std::vector<int> LipcPowerManager::readPausedPids() {
    std::vector<int> pids;
    std::ifstream in(m_pauseFile);
    if (!in.is_open()) return pids;

    std::string line;
    while (std::getline(in, line)) {
        if (!line.empty()) {
            try {
                pids.push_back(std::stoi(line));
            } catch (...) {}
        }
    }
    return pids;
}

void LipcPowerManager::writePausedPids(const std::vector<int>& pids) {
    std::ofstream out(m_pauseFile);
    if (out.is_open()) {
        for (int pid : pids) {
            out << pid << "\n";
        }
    }
}

std::vector<int> LipcPowerManager::scanFbUsers() {
    std::vector<int> users;
    DIR* proc = opendir("/proc");
    if (!proc) return users;

    pid_t myPid = getpid();
    struct dirent* entry = nullptr;
    while ((entry = readdir(proc)) != nullptr) {
        if (entry->d_type != DT_DIR) continue;
        int pid = 0;
        try {
            pid = std::stoi(entry->d_name);
        } catch (...) {
            continue;
        }
        if (pid <= 1 || pid == myPid) continue;

        std::string fdDirPath = "/proc/" + std::to_string(pid) + "/fd";
        DIR* fdDir = opendir(fdDirPath.c_str());
        if (!fdDir) continue;

        struct dirent* fdEntry = nullptr;
        while ((fdEntry = readdir(fdDir)) != nullptr) {
            std::string linkPath = fdDirPath + "/" + fdEntry->d_name;
            char target[256];
            ssize_t len = readlink(linkPath.c_str(), target, sizeof(target) - 1);
            if (len > 0) {
                target[len] = '\0';
                if (std::string(target) == "/dev/fb0") {
                    users.push_back(pid);
                    break;
                }
            }
        }
        closedir(fdDir);
    }
    closedir(proc);
    return users;
}

void LipcPowerManager::start(IPowerObserver* observer) {
    m_observer = observer;
    m_stopped = false;

    // 1. Spawning LIPC listener thread if lipc-wait-event is available
    if (access("/usr/bin/lipc-wait-event", X_OK) == 0) {
        m_threads.emplace_back(&LipcPowerManager::lipcEventLoop, this);
        Logger::info("Power", "Started LIPC power event listener");
    } else {
        Logger::info("Power", "lipc-wait-event not available, skipping LIPC listener");
    }

    // 2. Sleep watchdog
    if (access("/usr/bin/lipc-get-prop", X_OK) == 0) {
        m_threads.emplace_back(&LipcPowerManager::sleepWatchdogLoop, this);
        Logger::info("Power", "Started powerd sleep watchdog");
    }

    // 3. Scan power key
    glob_t g;
    if (glob("/dev/input/event*", GLOB_NOSORT, nullptr, &g) == 0) {
        for (size_t i = 0; i < g.gl_pathc; ++i) {
            int fd = ::open(g.gl_pathv[i], O_RDONLY | O_NONBLOCK);
            if (fd >= 0) {
                unsigned long keyBits[(KEY_MAX + 1) / (sizeof(unsigned long) * 8) + 1] = {0};
                if (::ioctl(fd, EVIOCGBIT(EV_KEY, sizeof(keyBits)), keyBits) >= 0) {
                    bool hasPower = (keyBits[KEY_POWER / (sizeof(unsigned long) * 8)] & (1UL << (KEY_POWER % (sizeof(unsigned long) * 8)))) != 0;
                    if (hasPower) {
                        std::string pth = g.gl_pathv[i];
                        m_threads.emplace_back(&LipcPowerManager::powerKeyLoop, this, pth);
                        Logger::info("Power", "Listening to power key: " + pth);
                    }
                }
                ::close(fd);
            }
        }
        globfree(&g);
    }
}

void LipcPowerManager::stop() {
    m_stopped = true;
    if (m_lipcWaitPid > 0) {
        kill(m_lipcWaitPid, SIGTERM);
        waitpid(m_lipcWaitPid, nullptr, WNOHANG);
        m_lipcWaitPid = -1;
    }
    for (auto& t : m_threads) {
        if (t.joinable()) {
            t.join();
        }
    }
    m_threads.clear();
}

void LipcPowerManager::lipcEventLoop() {
    while (!m_stopped) {
        int pipefd[2];
        if (pipe(pipefd) < 0) {
            sleep(3);
            continue;
        }

        pid_t pid = fork();
        if (pid == 0) {
            ::close(pipefd[0]);
            ::dup2(pipefd[1], STDOUT_FILENO);
            ::close(pipefd[1]);
            execlp("lipc-wait-event", "lipc-wait-event", "-m", "com.lab126.powerd",
                   "goingToScreenSaver,outOfScreenSaver", nullptr);
            _exit(1);
        } else if (pid > 0) {
            m_lipcWaitPid = pid;
            ::close(pipefd[1]);
            FILE* stream = fdopen(pipefd[0], "r");
            char buf[256];
            while (stream && fgets(buf, sizeof(buf), stream)) {
                if (m_stopped) break;
                std::string line(buf);
                if (line.find("goingToScreenSaver") != std::string::npos) {
                    Logger::info("Power", "Captured goingToScreenSaver");
                    std::lock_guard<std::mutex> lock(m_mutex);
                    suspendLocked();
                } else if (line.find("outOfScreenSaver") != std::string::npos) {
                    Logger::info("Power", "Captured outOfScreenSaver");
                    std::lock_guard<std::mutex> lock(m_mutex);
                    resumeLocked();
                }
            }
            if (stream) fclose(stream);
            waitpid(pid, nullptr, 0);
            m_lipcWaitPid = -1;
        } else {
            ::close(pipefd[0]);
            ::close(pipefd[1]);
        }
        if (!m_stopped) sleep(1);
    }
}

void LipcPowerManager::sleepWatchdogLoop() {
    int streak = 0;
    while (!m_stopped) {
        sleep(2);
        if (m_stopped || !m_isSleeping) {
            streak = 0;
            continue;
        }

        // Query lipc-get-prop com.lab126.powerd state
        FILE* fp = popen("lipc-get-prop com.lab126.powerd state 2>/dev/null", "r");
        if (!fp) continue;
        char buf[64];
        std::string state;
        if (fgets(buf, sizeof(buf), fp)) {
            state = buf;
        }
        pclose(fp);

        bool inSleep = (state.find("screensaver") != std::string::npos || state.find("suspend") != std::string::npos);
        if (inSleep) {
            streak = 0;
        } else {
            streak++;
            if (streak == 1) {
                Logger::info("Power", "Powerd active during sleep, requesting screensaver again");
                system("lipc-set-prop -i com.lab126.powerd powerButton 1 >/dev/null 2>&1");
            } else if (streak >= 2) {
                Logger::warn("Power", "Powerd active twice during sleep, self-healing resume");
                std::lock_guard<std::mutex> lock(m_mutex);
                resumeLocked();
                streak = 0;
            }
        }
    }
}

void LipcPowerManager::powerKeyLoop(const std::string& devicePath) {
    int fd = ::open(devicePath.c_str(), O_RDONLY);
    if (fd < 0) return;

    struct input_event ev{};
    while (!m_stopped) {
        ssize_t n = ::read(fd, &ev, sizeof(ev));
        if (n <= 0) {
            if (m_stopped) break;
            usleep(10000);
            continue;
        }

        if (ev.type == EV_KEY && (ev.code == KEY_POWER || ev.code == 356 /* KEY_POWER2 */) && ev.value == 1) {
            auto now = std::chrono::steady_clock::now();
            double nowS = std::chrono::duration<double>(now.time_since_epoch()).count();
            if (nowS - m_lastPowerPressAt < 0.8) continue;
            m_lastPowerPressAt = nowS;

            Logger::info("Power", "Physical power key pressed");
            handlePowerKey();
        }
    }
    ::close(fd);
}

void LipcPowerManager::handlePowerKey() {
    std::lock_guard<std::mutex> lock(m_mutex);
    if (m_isSleeping) {
        resumeLocked();
    } else {
        suspendLocked();
    }
}

void LipcPowerManager::suspendLocked() {
    if (m_isSleeping) return;
    m_isSleeping = true;
    Logger::info("Power", "Suspending: releasing touchscreen and resuming system UI...");

    if (m_observer) {
        m_observer->onSuspend();
    }

    auto pids = readPausedPids();
    if (pids.empty()) {
        pids = m_trackedPids.empty() ? scanFbUsers() : m_trackedPids;
    }
    m_trackedPids = pids;

    for (int pid : pids) {
        kill(pid, SIGCONT);
    }
    Logger::info("Power", "Sent SIGCONT to " + std::to_string(pids.size()) + " system processes");
}

void LipcPowerManager::resumeLocked() {
    if (!m_isSleeping) return;
    Logger::info("Power", "Resuming: re-pausing system UI and claiming touchscreen...");

    usleep(350000); // 350ms delay for kernel/powerd recovery

    auto pids = m_trackedPids.empty() ? readPausedPids() : m_trackedPids;
    if (pids.empty()) pids = scanFbUsers();

    std::vector<int> paused;
    for (int pid : pids) {
        if (kill(pid, SIGSTOP) == 0) {
            paused.push_back(pid);
        }
    }
    writePausedPids(paused);
    m_trackedPids = paused;
    Logger::info("Power", "Sent SIGSTOP to " + std::to_string(paused.size()) + " system processes");

    m_isSleeping = false;
    if (m_observer) {
        m_observer->onResume();
    }
    Logger::info("Power", "Resume complete");
}

} // namespace kinnovel::hal
