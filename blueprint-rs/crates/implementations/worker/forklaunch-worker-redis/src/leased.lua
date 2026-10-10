-- All keys for a tenant use one hash tag, including mixed-queue batch writes.
-- Redis scripts do not roll back runtime errors: validate types and duplicate
-- identities before making any writes.
local operation = ARGV[1]
local input = cjson.decode(ARGV[2])
local expected = {'hash', 'zset', 'zset', 'string'}
for index,key in ipairs(KEYS) do
    local actual = redis.call('TYPE', key).ok
    if actual ~= 'none' and actual ~= expected[((index-1)%4)+1] then
        return cjson.encode({error='corrupt'})
    end
end
local time = redis.call('TIME')
local now = tonumber(time[1])*1000 + math.floor(tonumber(time[2])/1000)
local function response(value) return cjson.encode({value=value}) end
local function load(id, base)
    local value = redis.call('HGET', KEYS[base+1], id)
    if not value then return nil end
    return cjson.decode(value)
end
local function save(job, base)
    redis.call('HSET', KEYS[base+1], job.id, cjson.encode(job))
end
local function clear_lease(job)
    job.token = cjson.null
    job.lease_until = cjson.null
end
local function recover_expired()
    -- Bound maintenance per operation so abandoned queues cannot monopolize Redis.
    for _,id in ipairs(redis.call('ZRANGEBYSCORE', KEYS[3], '-inf', now, 'LIMIT', 0, 128)) do
        local job = load(id, 0)
        if not job then return false end
        redis.call('ZREM', KEYS[3], id)
        clear_lease(job)
        if job.attempts >= job.max_attempts then
            job.status = 'failed'
            job.last_error = 'lease expired after final attempt'
        else
            job.status = 'queued'
            redis.call('ZADD', KEYS[2], job.available_at, job.member)
        end
        save(job, 0)
    end
    return true
end
if operation == 'enqueue' then
    local counts = {}
    local identities = {}
    for _,job in ipairs(input.jobs) do
        local base = job.key_index*4
        local identity = tostring(base)..':'..job.id
        if identities[identity] or redis.call('HEXISTS', KEYS[base+1], job.id) == 1 then
            return cjson.encode({error='duplicate'})
        end
        identities[identity] = true
        counts[base] = (counts[base] or 0)+1
    end
    -- Validate every sequence before incrementing any of them.
    for base,count in pairs(counts) do
        local value = redis.call('GET', KEYS[base+4])
        local number = 0
        if value then number = tonumber(value) end
        if not number or number < 0 or number ~= math.floor(number) or number+count >= 9007199254740991 then
            return cjson.encode({error='corrupt'})
        end
    end
    local positions = {}
    for base,count in pairs(counts) do
        positions[base] = redis.call('INCRBY', KEYS[base+4], count)-count
    end
    for _,job in ipairs(input.jobs) do
        local base = job.key_index*4
        positions[base] = positions[base]+1
        job.key_index = nil
        job.member = string.format('%020d', positions[base])..':'..job.id
        job.available_at = now
        save(job, base)
        redis.call('ZADD', KEYS[base+2], now, job.member)
    end
    return response(true)
elseif operation == 'claim' or operation == 'peek' then
    if not recover_expired() then return cjson.encode({error='corrupt'}) end
    local members = redis.call('ZRANGEBYSCORE', KEYS[2], '-inf', now, 'LIMIT', 0, input.limit or 1)
    if operation == 'peek' then
        local jobs = {}
        for _,member in ipairs(members) do
            local job = load(string.sub(member,22), 0)
            if not job then return cjson.encode({error='corrupt'}) end
            table.insert(jobs, job)
        end
        -- cjson encodes an empty Lua table as {}, not [].
        if #jobs == 0 then return '{"value":[]}' end
        return response(jobs)
    end
    if #members == 0 then return response(cjson.null) end
    local member = members[1]
    local job = load(string.sub(member,22), 0)
    if not job then return cjson.encode({error='corrupt'}) end
    job.status = 'processing'
    job.attempts = job.attempts+1
    job.token = input.token
    job.lease_until = now+input.duration
    redis.call('ZREM', KEYS[2], member)
    redis.call('ZADD', KEYS[3], job.lease_until, job.id)
    save(job, 0)
    return response(job)
elseif operation == 'stats' then
    return response({queued=redis.call('ZCARD',KEYS[2]), processing=redis.call('ZCARD',KEYS[3])})
elseif operation == 'inspect' then
    return response(load(input.id, 0) or cjson.null)
elseif operation == 'ack' or operation == 'fail' or operation == 'renew' then
    local job = load(input.id, 0)
    if not job or job.status ~= 'processing' or job.token ~= input.token or job.lease_until <= now then
        return response(false)
    end
    if operation == 'renew' then
        job.lease_until = math.max(job.lease_until, now+input.duration)
        redis.call('ZADD',KEYS[3],job.lease_until,job.id)
        save(job,0)
        return response(true)
    end
    redis.call('ZREM',KEYS[3],job.id)
    clear_lease(job)
    if operation == 'ack' then
        job.status = 'completed'
        save(job,0)
        return response(true)
    end
    job.last_error = input.reason
    if job.attempts < job.max_attempts then
        job.status = 'queued'
        job.available_at = now+job.backoff_ms
        redis.call('ZADD',KEYS[2],job.available_at,job.member)
    else
        job.status = 'failed'
    end
    save(job,0)
    return response(job.status)
end
return cjson.encode({error='invalid'})
