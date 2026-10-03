#pragma once

#include <functional>

namespace kinnovel::hal {

class IPowerObserver {
public:
    virtual ~IPowerObserver() = default;
    virtual void onSuspend() = 0;
    virtual void onResume() = 0;
};

class IPower {
public:
    virtual ~IPower() = default;

    virtual void start(IPowerObserver* observer) = 0;
    virtual void stop() = 0;
    virtual bool isSleeping() const = 0;
    virtual void handlePowerKey() = 0;
};

} // namespace kinnovel::hal
