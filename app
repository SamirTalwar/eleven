#!/usr/bin/env ruby

require 'fileutils'
require 'json'
require 'open3'
require 'pathname'
require 'socket'
require 'timeout'
require 'tmpdir'
require 'yaml'

DEBUG=ENV.include?('DEBUG')

class App
  def initialize(app_file)
    @app_file = app_file
    @dir = Pathname.new(Dir.mktmpdir('eleven'))
    @socket_dir = @dir + 'sockets'
    @config_dir = @dir + 'config'
    @socket_dir.mkdir
    @config_dir.mkdir
  end

  def run!
    debug "Application: #{@app_file}"
    debug "Directory: #{@dir}"

    processes, sockets = configure()
    debug "Processes: #{JSON.pretty_generate(processes)}"
    debug

    @running = true
    pids = start processes, sockets
    begin
      pids.each { |pid| Process.wait(pid) }
    rescue Interrupt
    ensure
      stop pids, sockets
    end
  ensure
    tear_down
  end

  def configure
    configuration = YAML.load(File.read(@app_file))
    sockets = {}
    processes = configuration['processes']
      .group_by { |process| process['name'] }
      .map { |name, ps|
        process = ps[0]
        socket = UNIXServer.new((@socket_dir + "#{name}.sock").to_s)
        sockets[name] = socket
        [name, process]
      }
      .map { |name, process|
        command = process['command']
        config = reference_sockets(process['config'], sockets)
        [name, command, config]
      }
    [processes, sockets]
  end

  def start(processes, sockets)
    pids = []
    processes.each do |name, command, config|
      config_file = @config_dir + "#{name}.config"
      config_file.open('w') do |f|
        JSON.dump(config, f)
      end
      server = sockets[name]
      Thread.new do
        Open3.popen2(command, config_file.to_s) { |stdin, stdout, wait_thr|
          pids << wait_thr.pid
          while @running
            begin
              client = server.accept_nonblock
              stdin.puts(client.gets)
              client.puts(stdout.gets)
              client.close
            rescue IO::WaitReadable, Errno::EINTR
              IO.select([server])
            end
          end
          stdin.close
          stdout.close
          wait_thr.wait
        }
      end
    end
    pids
  end

  def stop(pids, sockets)
    $stderr.puts 'Stopping...'
    @running = false

    pids.each do |pid|
      begin
        Process.kill 0, pid
      rescue Errno::ESRCH
        next
      end

      begin
        Process.kill 'TERM', pid
        begin
          Timeout.timeout 1 do
            Process.wait pid
          end
        rescue Errno::ECHILD
        rescue Timeout::Error
          $stderr.puts "Forcefully terminating #{pid}..."
          Process.kill 'KILL', pid
          begin
            Process.wait pid
          rescue Errno::ECHILD
          end
        end
      rescue StandardError => error
        $stderr.puts "Failed to kill PID #{pid}. #{error.class}: #{error.message}"
      end
    end

    sockets.each do |name, socket|
      socket.close
    end

    $stderr.puts 'Stopped.'
  end

  def tear_down
    FileUtils.rm_r @dir
  end

  def reference_sockets(config, sockets)
    config.each do |key, value|
      if value.is_a?(Hash)
        reference_sockets(value, sockets)
      elsif key == 'process'
        config[key] = sockets[value].path
      end
    end
  end

  def debug(*strings)
    $stderr.puts(*strings) if DEBUG
  end
end

if __FILE__ == $0
  app_file = ARGV[0]
  App.new(app_file).run!
end
