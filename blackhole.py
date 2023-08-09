import socket
import random
import threading
import time
import os
import argparse
from time import sleep

ip_counter = {}
last_request_time_by_ip = {}
fake_responses = []

### Read files from vulnerability-responses directory into fake_responses list
for filename in os.listdir('responses'):
    with open('responses/' + filename, 'r') as f:
        fake_responses.append(f.read())


def handle_client(client_socket, client_address):
    print(f"Connection received from {client_address}")
    handle_delay(client_address[0])
    try:
        ### You can now interact with the client using client_socket, e.g.:
        message = fake_vulnerability_response()
        client_socket.sendall(message.encode('utf-8'))
    finally:
        ### Close the client socket when done
        client_socket.close()


### Create a server sockets on random ports in a range, or from a list of ports
def dynamic_port_server(min_port=10000, max_port=65535, port_list=[]):
    running = True    
    while running:
        server_sockets = [] ### List to store the 10 server sockets
        try:
            ### Create and run 10 server sockets
            for _ in range(10):
                port = -1
                if len(port_list) == 0:
                    port = random.randint(min_port, max_port)
                else:
                    ### Select random port from port_list
                    port = random.choice(port_list)

                server_socket = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
                server_socket.bind(('0.0.0.0', port))
                server_socket.listen(1)
                server_socket.settimeout(1)
                print(f"Listening on port {port}...")
                server_sockets.append(server_socket)
            
            ### Keep track of the time when the server started
            curr_time = time.time()

            ### Run on current ports for 5 seconds
            while time.time() - curr_time < 5:
                for server_socket in server_sockets:
                    try:
                        client_socket, client_address = server_socket.accept()
                    except socket.timeout:
                        continue

                    ### Start a new thread to handle the connection
                    client_thread = threading.Thread(target=handle_client, args=(client_socket, client_address), daemon=True)
                    client_thread.start()
        except KeyboardInterrupt:
            print("Closing server sockets...")
            ### Close all server sockets
            for server_socket in server_sockets:
                server_socket.close()
            running = False
            break
        except Exception as e:
            pass



### Delay the response based on the number of requests from the IP
def rate_based_delay(ip):
    delay = ip_counter.get(ip, 0)
    
    ### Increase delay by 1 second to a maximum of 10 seconds
    ip_counter[ip] = min(delay + 1, 10)
    if delay > 0:
        sleep(delay)


### For every 20 seconds it's been since the last request from IP, decrease the delay by 1 second
def rate_based_delay_decay(ip):
    ip_counter[ip] = max(ip_counter.get(ip, 0) - int((time.time() - last_request_time_by_ip[ip]) / 20), 0)
    

def handle_delay(ip):
    last_request_time_by_ip[ip] = time.time()
    rate_based_delay_decay(ip)
    rate_based_delay(ip)


### Get a random response from the list of fake responses
def fake_vulnerability_response():        
    return random.choice(fake_responses)


def main():
    argparser = argparse.ArgumentParser(description='Blackhole server')

    argparser.add_argument('-m', '--min-port', type=int, default=None, help='Minimum port to listen on')
    argparser.add_argument('-M', '--max-port', type=int, default=None, help='Maximum port to listen on')
    argparser.add_argument('-l', '--port-list', nargs='+', default=[], help='List of ports to listen on')

    args = argparser.parse_args()

    ### Count how many port arguments were specified
    port_conditions = sum([
        bool(args.port_list), ### Check if the list is not empty
        (args.min_port is not None and args.max_port is not None),
        (args.min_port is None and args.max_port is None and not args.port_list)
    ])

    ### Check that exactly one port argument was specified
    if port_conditions != 1:
        argparser.error("Invalid port specification; only one of -p, -m/-M, or -l can be specified")

    dynamic_port_server(args.min_port, args.max_port, args.port_list)


if __name__ == '__main__':
    main()